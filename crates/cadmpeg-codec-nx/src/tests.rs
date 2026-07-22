// SPDX-License-Identifier: Apache-2.0
//! Tests over synthetic byte fixtures. No real CAD file exists in this repo and
//! none may be added, so every fixture is a hand-built `.prt` byte image whose
//! bytes exercise the real SPLMSSTR container parse, the Parasolid zlib
//! extraction/classification, and the analytic geometry decode, and fail if the
//! code regresses.
#![allow(clippy::unwrap_used)]

use std::io::{Cursor, Write};

use flate2::write::ZlibEncoder;
use flate2::Compression;

use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};
use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, CurveGeometry, PcurveGeometry, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Vector3};
use cadmpeg_ir::report::LossCategory;
use cadmpeg_ir::Exactness;

use crate::container;
use crate::parasolid::{self, StreamKind};
use crate::NxCodec;

const MAGIC: &[u8; 8] = b"SPLMSSTR";

fn shifted_f64_bytes(value: f64) -> [u8; 8] {
    let mut bytes = value.to_be_bytes();
    bytes[0] -= 0x10;
    bytes
}

fn attach_test_body_surface(
    ir: &mut cadmpeg_ir::document::CadIr,
    body_id: &cadmpeg_ir::ids::BodyId,
    surface: cadmpeg_ir::ids::SurfaceId,
) {
    use cadmpeg_ir::ids::{FaceId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Body, BodyKind, Face, Region, Sense, Shell};

    let region_id = RegionId(format!("{}:region", body_id.0));
    let shell_id = ShellId(format!("{}:shell", body_id.0));
    if !ir.model.bodies.iter().any(|body| body.id == *body_id) {
        ir.model.bodies.push(Body {
            id: body_id.clone(),
            kind: BodyKind::Solid,
            regions: vec![region_id.clone()],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id.clone()],
        });
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id,
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
    }
    let face_id = FaceId(format!("{}:face#{}", body_id.0, ir.model.faces.len()));
    ir.model
        .shells
        .iter_mut()
        .find(|shell| shell.id == shell_id)
        .unwrap()
        .faces
        .push(face_id.clone());
    ir.model.faces.push(Face {
        id: face_id,
        shell: shell_id,
        surface,
        sense: Sense::Forward,
        loops: Vec::new(),
        name: None,
        color: None,
        tolerance: None,
    });
}

#[test]
fn display_jt_index_requires_every_declared_header() {
    use crate::container::{Container, DirEntry, Region};

    let mut inflated = Vec::new();
    inflated.extend_from_slice(&24_u32.to_le_bytes());
    inflated.extend_from_slice(&[3; 16]);
    inflated.push(1);
    inflated.extend_from_slice(&5_u32.to_le_bytes());
    inflated.extend_from_slice(&[9, 8, 7]);
    inflated.extend_from_slice(&16_u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);
    inflated.extend_from_slice(&[6, 5]);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&inflated).unwrap();
    let compressed = encoder.finish().unwrap();
    let segment_byte_len = 24 + 9 + compressed.len() as u32;
    let mut data = Vec::new();
    data.extend_from_slice(&9_u32.to_le_bytes());
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&100_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&28_u32.to_le_bytes());
    data.extend_from_slice(&[0; 4]);
    let mut version = [b' '; 80];
    version[..14].copy_from_slice(b"Version 9.4 JT");
    data.extend_from_slice(&version);
    data.push(0);
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&105_u32.to_le_bytes());
    data.extend_from_slice(&[1; 16]);
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&137_u32.to_le_bytes());
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&1_u32.to_be_bytes());
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&2_u32.to_le_bytes());
    data.extend_from_slice(&(compressed.len() as u32 + 1).to_le_bytes());
    data.push(2);
    data.extend_from_slice(&compressed);
    let container = Container {
        data: data.clone(),
        version: 6,
        file_tag: 0,
        footer_offset: 0,
        entries: vec![DirEntry {
            name: "/Root/UG_PART/DisplayJT".to_string(),
            region: Region::Footer,
            file_span: Some((0, data.len() as u64)),
        }],
    };
    let indices = crate::native::display_jt_indices(&container);
    assert_eq!(indices[0].version, 9);
    assert_eq!(indices[0].declared_count, 1);
    assert_eq!(indices[0].rows[0].header_offset, 28);
    assert_eq!(indices[0].rows[0].value, 100);
    let documents = crate::native::display_jt_documents(&container, &indices);
    assert_eq!(
        (documents[0].format_major, documents[0].format_minor),
        (9, 4)
    );
    assert_eq!(documents[0].toc_offset, 105);
    assert_eq!(
        documents[0].physical_byte_len,
        137 + u64::from(segment_byte_len)
    );
    assert_eq!(documents[0].toc_entries.len(), 1);
    assert_eq!(documents[0].toc_entries[0].segment_offset, 137);
    assert_eq!(
        documents[0].toc_entries[0].segment_byte_len,
        segment_byte_len
    );
    assert_eq!(documents[0].toc_entries[0].attributes, [0, 0, 0, 1]);
    let segments = crate::native::display_jt_segments(&container, &documents);
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].id.matches('#').count(), 1);
    assert!(!segments[0].id.contains(&documents[0].id));
    assert_eq!(segments[0].segment_type, 1);
    assert_eq!(segments[0].segment_byte_len, segment_byte_len);
    let compression = segments[0].compression.as_ref().unwrap();
    assert_eq!(
        compression.compressed_data_byte_len,
        compressed.len() as u32 + 1
    );
    assert_eq!(compression.compressed_byte_len, compressed.len() as u32);
    assert_eq!(
        compression.inflated_sha256,
        cadmpeg_ir::hash::sha256_hex(&inflated)
    );
    let (compressed_elements, sequences) =
        crate::native::display_jt_compressed_element_sequences(&container, &segments);
    assert_eq!(compressed_elements.len(), 1);
    assert_eq!(compressed_elements[0].segment_type, 1);
    assert_eq!(compressed_elements[0].object_type_id, [3; 16]);
    assert_eq!(compressed_elements[0].object_id, 5);
    assert_eq!(compressed_elements[0].object_base_type, 1);
    assert_eq!(compressed_elements[0].body_byte_len, 3);
    assert_eq!(sequences.len(), 1);
    assert_eq!(sequences[0].framed_byte_len, 48);
    assert_eq!(sequences[0].tail, [6, 5]);

    let mut malformed_compression = container.clone();
    malformed_compression.data[193..197]
        .copy_from_slice(&(compressed.len() as u32 + 2).to_le_bytes());
    assert!(crate::native::display_jt_segments(&malformed_compression, &documents).is_empty());

    let mut malformed = container;
    malformed.data[28] = b'X';
    assert!(crate::native::display_jt_indices(&malformed).is_empty());
}

#[test]
fn display_jt_shape_lod_requires_canonical_end_marker_and_tail() {
    use crate::container::Container;
    use crate::native::DisplayJtSegment;

    let object_type_id = [0x5a; 16];
    let body = [9, 8, 7];
    let mut data = Vec::new();
    data.extend_from_slice(&[1; 16]);
    data.extend_from_slice(&7_u32.to_le_bytes());
    data.extend_from_slice(&78_u32.to_le_bytes());
    data.extend_from_slice(&24_u32.to_le_bytes());
    data.extend_from_slice(&object_type_id);
    data.push(4);
    data.extend_from_slice(&42_u32.to_le_bytes());
    data.extend_from_slice(&body);
    data.extend_from_slice(&16_u32.to_le_bytes());
    data.extend_from_slice(&[0xff; 16]);
    data.extend_from_slice(&[1, 0, 0, 0, 0, 0]);
    let container = Container {
        data,
        version: 6,
        file_tag: 0,
        footer_offset: 0,
        entries: Vec::new(),
    };
    let segment = DisplayJtSegment {
        id: "segment".to_string(),
        document: "document".to_string(),
        toc_entry: "entry".to_string(),
        segment_id: vec![1; 16],
        segment_type: 7,
        segment_byte_len: 78,
        payload_sha256: String::new(),
        compression: None,
        source_offset: 0,
    };
    let elements =
        crate::native::display_jt_shape_lod_elements(&container, std::slice::from_ref(&segment));
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].object_type_id, object_type_id);
    assert_eq!(elements[0].object_id, 42);
    assert_eq!(elements[0].object_base_type, 4);
    assert_eq!(elements[0].body_byte_len, 3);

    let mut malformed = container;
    *malformed.data.last_mut().unwrap() = 1;
    assert!(crate::native::display_jt_shape_lod_elements(&malformed, &[segment]).is_empty());
}

#[test]
fn display_jt_shape_lod_binding_resolves_property_table_segment_reference() {
    use crate::container::Container;
    use crate::native::DisplayJtSegment;

    let mut inflated = Vec::new();
    inflated.extend_from_slice(&16_u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);

    let mut late_body = vec![1, 0];
    late_body.extend_from_slice(&0x4000_0000_u32.to_le_bytes());
    late_body.extend_from_slice(&1_u16.to_le_bytes());
    late_body.extend_from_slice(&[9; 16]);
    late_body.extend_from_slice(&7_u32.to_le_bytes());
    late_body.extend_from_slice(&12_u32.to_le_bytes());
    late_body.extend_from_slice(&1_u32.to_le_bytes());
    inflated.extend_from_slice(&57_u32.to_le_bytes());
    inflated.extend_from_slice(&[
        0xe5, 0x5b, 0xb0, 0xe0, 0xbd, 0xfb, 0xd1, 0x11, 0xa3, 0xa7, 0x00, 0xaa, 0x00, 0xd1, 0x09,
        0x54,
    ]);
    inflated.push(8);
    inflated.extend_from_slice(&3_u32.to_le_bytes());
    inflated.extend_from_slice(&late_body);

    let key = "JT_LLPROP_SHAPEIMPL";
    let mut string_body = vec![1, 0, 0, 0, 0, 0x40, 1, 0];
    string_body.extend_from_slice(&(key.len() as u32).to_le_bytes());
    for unit in key.encode_utf16() {
        string_body.extend_from_slice(&unit.to_le_bytes());
    }
    inflated.extend_from_slice(&(21_u32 + string_body.len() as u32).to_le_bytes());
    inflated.extend_from_slice(&[
        0x6e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ]);
    inflated.push(5);
    inflated.extend_from_slice(&4_u32.to_le_bytes());
    inflated.extend_from_slice(&string_body);
    inflated.extend_from_slice(&16_u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);
    inflated.extend_from_slice(&1_u16.to_le_bytes());
    inflated.extend_from_slice(&1_u32.to_le_bytes());
    inflated.extend_from_slice(&2_u32.to_le_bytes());
    inflated.extend_from_slice(&4_u32.to_le_bytes());
    inflated.extend_from_slice(&3_u32.to_le_bytes());
    inflated.extend_from_slice(&0_u32.to_le_bytes());

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&inflated).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut data = vec![0; 33];
    data.extend_from_slice(&compressed);
    let container = Container {
        data,
        version: 6,
        file_tag: 0,
        footer_offset: 0,
        entries: Vec::new(),
    };
    let scene = DisplayJtSegment {
        id: "scene".into(),
        document: "document".into(),
        toc_entry: "scene-entry".into(),
        segment_id: vec![1; 16],
        segment_type: 1,
        segment_byte_len: (33 + compressed.len()) as u32,
        payload_sha256: String::new(),
        compression: None,
        source_offset: 0,
    };
    let shape = DisplayJtSegment {
        id: "shape".into(),
        document: "document".into(),
        toc_entry: "shape-entry".into(),
        segment_id: vec![9; 16],
        segment_type: 7,
        segment_byte_len: 0,
        payload_sha256: String::new(),
        compression: None,
        source_offset: 0,
    };
    let bindings = crate::native::display_jt_shape_lod_bindings(&container, &[scene, shape]);
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].shape_node_object_id, 2);
    assert_eq!(bindings[0].shape_segment, "shape");
    assert_eq!(bindings[0].payload_object_id, 12);
    assert_eq!(bindings[0].key, key);
}

#[test]
fn display_jt_string_property_body_requires_exact_utf16_frame() {
    let mut body = vec![1, 0, 0, 0, 0, 0x40, 1, 0];
    body.extend_from_slice(&3_u32.to_le_bytes());
    body.extend_from_slice(&[b'N', 0, b'X', 0, 0xa9, 0x03]);
    let (units, value) = crate::native::parse_jt_string_property_atom_body(&body).unwrap();
    assert_eq!(units, [0x4e, 0x58, 0x3a9]);
    assert_eq!(value, "NXΩ");

    body.push(0);
    assert!(crate::native::parse_jt_string_property_atom_body(&body).is_none());
}

#[test]
fn display_jt9_tri_strip_header_requires_supported_versions() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0x4a_u64.to_le_bytes());
    body.extend_from_slice(&2_u16.to_le_bytes());
    body.extend_from_slice(&0x1234_u32.to_le_bytes());
    body.extend_from_slice(&2_u16.to_le_bytes());
    body.extend_from_slice(&[9, 8, 7]);
    let (bindings, mesh_version, records_id, compressed_version, compressed) =
        crate::native::parse_jt9_tri_strip_lod_header(&body).unwrap();
    assert_eq!(bindings, 0x4a);
    assert_eq!(mesh_version, 2);
    assert_eq!(records_id, 0x1234);
    assert_eq!(compressed_version, 2);
    assert_eq!(compressed, [9, 8, 7]);

    body[12..14].copy_from_slice(&3_u16.to_le_bytes());
    assert!(crate::native::parse_jt9_tri_strip_lod_header(&body).is_none());
}

#[test]
fn jt_int32_cdp2_decodes_empty_and_bitlength_packets() {
    assert_eq!(
        crate::jt::decode_int32_cdp2(&[0, 0, 0, 0], 0),
        Some((vec![], 4))
    );

    let encode_packet = |bits: &[u8], value_count: u32| {
        let mut code_words = Vec::new();
        for chunk in bits.chunks(32) {
            let mut word = 0u32;
            for bit in chunk {
                word = (word << 1) | u32::from(*bit);
            }
            word <<= 32 - chunk.len();
            code_words.extend_from_slice(&word.to_le_bytes());
        }
        let mut packet = value_count.to_le_bytes().to_vec();
        packet.push(1);
        packet.extend_from_slice(&(bits.len() as u32).to_le_bytes());
        packet.extend(code_words);
        packet
    };
    let field = |bits: &mut Vec<u8>, value: u32, width: u8| {
        bits.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
    };

    // Fixed-width mode: range [-1, 1], followed by codes for 1 and -1.
    let mut bits = vec![0];
    field(&mut bits, 2, 6);
    field(&mut bits, 2, 6);
    field(&mut bits, 0b11, 2);
    field(&mut bits, 0b01, 2);
    field(&mut bits, 2, 2);
    field(&mut bits, 0, 2);
    let packet = encode_packet(&bits, 2);
    assert_eq!(
        crate::jt::decode_int32_cdp2(&packet, 0),
        Some((vec![1, -1], packet.len()))
    );

    // Variable-width mode: mean 10, one two-bit run containing +1 and -1.
    let mut bits = vec![1];
    field(&mut bits, 10, 32);
    field(&mut bits, 3, 3);
    field(&mut bits, 3, 3);
    field(&mut bits, 2, 3);
    field(&mut bits, 2, 3);
    field(&mut bits, 1, 2);
    field(&mut bits, 3, 2);
    let packet = encode_packet(&bits, 2);
    assert_eq!(
        crate::jt::decode_int32_cdp2(&packet, 0),
        Some((vec![11, 9], packet.len()))
    );
}

#[test]
fn jt_int32_cdp2_decodes_arithmetic_context_with_zero_frequency_entry() {
    let mut context_bits = Vec::<bool>::new();
    let mut push = |value: u32, width: u8| {
        for shift in (0..width).rev() {
            context_bits.push((value >> shift) & 1 != 0);
        }
    };
    push(2, 6);
    push(1, 6);
    push(1, 6);
    push(7, 32);
    push(0, 2);
    push(0, 1);
    push(0, 1);
    push(1, 2);
    push(1, 1);
    push(0, 1);
    let mut context = vec![0, 2];
    for chunk in context_bits.chunks(8) {
        let mut byte = 0u8;
        for bit in chunk {
            byte = (byte << 1) | u8::from(*bit);
        }
        byte <<= 8 - chunk.len();
        context.push(byte);
    }
    let mut packet = Vec::new();
    packet.extend_from_slice(&3_u32.to_le_bytes());
    packet.push(3);
    packet.extend_from_slice(&16_u32.to_le_bytes());
    packet.extend_from_slice(&0_u32.to_le_bytes());
    packet.extend_from_slice(&context);
    packet.extend_from_slice(&0_u32.to_le_bytes());
    assert_eq!(
        crate::jt::decode_int32_cdp2(&packet, 0),
        Some((vec![7, 7, 7], packet.len()))
    );

    packet.truncate(packet.len() - 4);
    assert!(crate::jt::decode_int32_cdp2(&packet, 0).is_none());
}

#[test]
fn jt_int32_cdp2_decodes_unsplit_and_split_chopper_packets() {
    let nested = [2, 0, 0, 0, 1, 21, 0, 0, 0, 0x00, 0xc0, 0x16, 0x04];
    let low_bits = [2, 0, 0, 0, 1, 17, 0, 0, 0, 0x00, 0x80, 0x12, 0x04];
    let mut unsplit = vec![2, 0, 0, 0, 4, 0];
    unsplit.extend_from_slice(&nested);
    assert_eq!(
        crate::jt::decode_int32_cdp2(&unsplit, 0),
        Some((vec![1, -1], unsplit.len()))
    );

    let mut split = vec![2, 0, 0, 0, 4, 2];
    split.extend_from_slice(&10_i32.to_le_bytes());
    split.push(4);
    split.extend_from_slice(&nested);
    split.extend_from_slice(&low_bits);
    assert_eq!(
        crate::jt::decode_int32_cdp2(&split, 0),
        Some((vec![15, 7], split.len()))
    );
}

#[test]
fn jt_int32_cdp2_frames_zero_chop_nested_packet() {
    let nested = [2, 0, 0, 0, 1, 21, 0, 0, 0, 0x00, 0xc0, 0x16, 0x04];
    let mut packet = vec![2, 0, 0, 0, 4, 0];
    packet.extend_from_slice(&nested);
    assert_eq!(
        crate::jt::frame_int32_cdp2(&packet, 0),
        Some((2, 4, packet.len()))
    );

    packet[6] = 3;
    assert!(crate::jt::frame_int32_cdp2(&packet, 0).is_none());
}

#[test]
fn jt_predictors_reconstruct_primal_integers() {
    use crate::jt::{unpack_predictor_residuals, Predictor};

    let primers = [10, 20, 30, 40];
    let residuals = [10, 20, 30, 40, 5, -2];
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Lag1),
        [10, 20, 30, 40, 45, 43]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Lag2),
        [10, 20, 30, 40, 35, 38]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Stride1),
        [10, 20, 30, 40, 55, 68]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Stride2),
        [10, 20, 30, 40, 55, 58]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::StripIndex),
        [10, 20, 30, 40, 37, 40]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Ramp),
        [10, 20, 30, 40, 9, 3]
    );
    assert_eq!(
        unpack_predictor_residuals(&[10, 20, 30, 40, 0x2d ^ 0x28], Predictor::Xor1),
        [10, 20, 30, 40, 45]
    );
    assert_eq!(
        unpack_predictor_residuals(&[10, 20, 30, 40, 0x23 ^ 0x1e], Predictor::Xor2),
        [10, 20, 30, 40, 35]
    );
    assert_eq!(
        unpack_predictor_residuals(&residuals, Predictor::Null),
        residuals
    );
    assert_eq!(primers, residuals[..4]);
}

#[test]
fn jt_predictors_use_wrapping_i32_arithmetic() {
    use crate::jt::{unpack_predictor_residuals, Predictor};

    assert_eq!(
        unpack_predictor_residuals(&[0, 0, 0, i32::MAX, 1], Predictor::Lag1),
        [0, 0, 0, i32::MAX, i32::MIN]
    );
}

#[test]
fn jt_topological_dual_mesh_reconstructs_closed_tetrahedron() {
    let polygons = crate::jt_topology::decode(
        [&[3, 3, 3], &[3], &[], &[], &[], &[], &[], &[]],
        &[3, 3, 3, 3],
        &[10, 12, 11, 13],
        &[0, 0, 0, 0],
        &[],
        &[],
        crate::jt_topology::AttributeMaskLanes {
            small: [&[], &[1, 1, 1, 1], &[], &[], &[], &[], &[], &[]],
            context_7_next_30: &[],
            context_7_upper_4: &[],
            large_words: &[],
        },
    )
    .expect("valid closed dual mesh");

    assert_eq!(
        polygons
            .iter()
            .map(|polygon| polygon.vertex_indices.as_slice())
            .collect::<Vec<_>>(),
        vec![&[0, 1, 2], &[2, 1, 3], &[2, 3, 0], &[3, 1, 0]]
    );
    assert_eq!(
        polygons
            .iter()
            .map(|polygon| polygon.group)
            .collect::<Vec<_>>(),
        vec![10, 12, 11, 13]
    );
    assert_eq!(
        polygons[0].attribute_indices,
        vec![Some(0), Some(1), Some(2)]
    );
}

#[test]
fn jt_uniform_dequantization_uses_the_full_unsigned_code_range() {
    assert_eq!(
        crate::jt::dequantize_uniform(0, [10.0, 20.0], 2),
        Some(8.333_333)
    );
    assert_eq!(
        crate::jt::dequantize_uniform(3, [10.0, 20.0], 2),
        Some(18.333_334)
    );
    assert_eq!(crate::jt::dequantize_uniform(4, [10.0, 20.0], 2), None);
    assert_eq!(crate::jt::dequantize_uniform(-1, [4.0, 4.0], 32), Some(4.0));
}

#[test]
fn jt_quantized_coordinate_array_decodes_three_lag1_code_vectors() {
    let mut code = Vec::new();
    let mut push = |value: u32, width: u8| {
        code.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
    };
    push(0, 1);
    push(0, 6);
    push(3, 6);
    push(3, 3);
    for value in 0..4 {
        push(value, 2);
    }
    let mut word = 0u32;
    for bit in &code {
        word = (word << 1) | u32::from(*bit);
    }
    word <<= 32 - code.len();
    let mut packet = 4_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(&(code.len() as u32).to_le_bytes());
    packet.extend_from_slice(&word.to_le_bytes());
    let mut array = Vec::new();
    for _ in 0..3 {
        array.extend_from_slice(&packet);
    }
    array.extend_from_slice(&0x1234_5678_u32.to_le_bytes());

    let (points, hash, consumed) =
        crate::jt::decode_vertex_coordinates(&array, 4, [[10.0, 20.0]; 3], [2; 3])
            .expect("complete quantized coordinate array");
    assert_eq!(hash, 0x1234_5678);
    assert_eq!(consumed, array.len());
    assert_eq!(points[0], [8.333_333; 3]);
    assert_eq!(points[3], [18.333_334; 3]);
}

#[test]
fn jt_scene_binding_transfers_visible_triangles_in_document_units() {
    use crate::native::{
        DisplayJtBaseNodeData, DisplayJtCompressedElement, DisplayJtCompressedVertexRecordsHeader,
        DisplayJtGeometricTransformAttribute, DisplayJtGroupNodeData, DisplayJtInstanceNode,
        DisplayJtPolygonMesh, DisplayJtShapeLodBinding, DisplayJtShapeLodElement,
        DisplayJtTriStripShapeNode, DisplayJtVertexColors, DisplayJtVertexCoordinateArrayHeader,
        DisplayJtVertexCoordinates, DisplayJtVertexFlags, DisplayJtVertexNormals,
        DisplayJtVertexTextureCoordinates,
    };

    let mesh = DisplayJtPolygonMesh {
        id: "native-mesh".into(),
        topology: "topology".into(),
        coordinate_header: "coordinate-header".into(),
        polygons: vec![vec![0, 1, 2], vec![2, 1, 0, 2]],
        vertex_attribute_indices: vec![vec![Some(0), Some(1), Some(2)], vec![None; 4]],
        polygon_groups: vec![4, -1],
        polygon_flags: vec![0, 0],
        source_offset: 80,
    };
    let coordinates = DisplayJtVertexCoordinates {
        id: "coordinates".into(),
        header: "coordinate-header".into(),
        points_m: vec![[0.0, 0.0, 0.0], [0.001, 0.0, 0.0], [0.0, 0.002, 0.0]],
        coordinate_hash: 0,
        byte_len: 4,
        source_offset: 90,
    };
    let header = DisplayJtVertexCoordinateArrayHeader {
        id: "coordinate-header".into(),
        element: "shape-element".into(),
        unique_vertex_count: 3,
        component_count: 3,
        component_ranges: [[0.0, 0.0]; 3],
        component_quantization_bits: [0; 3],
        compressed_components_byte_len: 4,
        compressed_components_sha256: "00".repeat(32),
        source_offset: 60,
    };
    let shape_element = DisplayJtShapeLodElement {
        id: "shape-element".into(),
        segment: "shape-segment".into(),
        ordinal: 0,
        object_type_id: vec![0; 16],
        object_base_type: 4,
        object_id: 7,
        body_byte_len: 0,
        body_sha256: "00".repeat(32),
        source_offset: 100,
    };
    let binding = DisplayJtShapeLodBinding {
        id: "binding".into(),
        scene_segment: "scene-segment".into(),
        table_version: 1,
        shape_node_object_id: 9,
        key_object_id: 1,
        key: "JT_LLPROP_SHAPEIMPL".into(),
        value_object_id: 2,
        state_flags: 0,
        property_version: 1,
        shape_segment: "shape-segment".into(),
        payload_object_id: 7,
        reserved_value: 1,
        source_offset: 110,
    };
    let base = DisplayJtBaseNodeData {
        id: "base".into(),
        element: "scene-element".into(),
        object_type_id: vec![0; 16],
        object_id: 9,
        version: 1,
        flags: 0,
        attribute_object_ids: vec![10],
        family_data_byte_len: 0,
        family_data_sha256: "00".repeat(32),
        source_offset: 120,
    };
    let compressed = DisplayJtCompressedElement {
        id: "scene-element".into(),
        segment: "scene-segment".into(),
        segment_type: 1,
        ordinal: 0,
        object_type_id: vec![0; 16],
        object_base_type: 2,
        object_id: 9,
        body_byte_len: 0,
        body_sha256: "00".repeat(32),
        inflated_offset: 0,
        source_offset: 120,
    };
    let instance_base = DisplayJtBaseNodeData {
        id: "instance-base".into(),
        element: "instance-element".into(),
        object_type_id: vec![0; 16],
        object_id: 11,
        version: 1,
        flags: 0,
        attribute_object_ids: Vec::new(),
        family_data_byte_len: 6,
        family_data_sha256: "00".repeat(32),
        source_offset: 122,
    };
    let instance_element = DisplayJtCompressedElement {
        id: "instance-element".into(),
        segment: "scene-segment".into(),
        segment_type: 1,
        ordinal: 1,
        object_type_id: vec![0; 16],
        object_base_type: 0,
        object_id: 11,
        body_byte_len: 0,
        body_sha256: "00".repeat(32),
        inflated_offset: 0,
        source_offset: 122,
    };
    let instance = DisplayJtInstanceNode {
        id: "instance-node".into(),
        base_node: "instance-base".into(),
        object_id: 11,
        version: 1,
        child_object_id: 9,
        source_offset: 122,
    };
    let mut second_instance_base = instance_base.clone();
    second_instance_base.id = "second-instance-base".into();
    second_instance_base.element = "second-instance-element".into();
    second_instance_base.object_id = 12;
    second_instance_base.source_offset = 123;
    let mut second_instance_element = instance_element.clone();
    second_instance_element.id = "second-instance-element".into();
    second_instance_element.ordinal = 2;
    second_instance_element.object_id = 12;
    second_instance_element.source_offset = 123;
    let second_instance = DisplayJtInstanceNode {
        id: "second-instance-node".into(),
        base_node: "second-instance-base".into(),
        object_id: 12,
        version: 1,
        child_object_id: 9,
        source_offset: 123,
    };
    let group_base = DisplayJtBaseNodeData {
        id: "group-base".into(),
        element: "group-element".into(),
        object_type_id: vec![0; 16],
        object_id: 20,
        version: 1,
        flags: 0,
        attribute_object_ids: Vec::new(),
        family_data_byte_len: 14,
        family_data_sha256: "00".repeat(32),
        source_offset: 124,
    };
    let group_element = DisplayJtCompressedElement {
        id: "group-element".into(),
        segment: "scene-segment".into(),
        segment_type: 1,
        ordinal: 3,
        object_type_id: vec![0; 16],
        object_base_type: 1,
        object_id: 20,
        body_byte_len: 0,
        body_sha256: "00".repeat(32),
        inflated_offset: 0,
        source_offset: 124,
    };
    let group = DisplayJtGroupNodeData {
        id: "group-node".into(),
        base_node: "group-base".into(),
        object_id: 20,
        version: 1,
        child_object_ids: vec![11, 12],
        family_data_byte_len: 0,
        family_data_sha256: "00".repeat(32),
        source_offset: 124,
    };
    let mut ignored_group_base = group_base.clone();
    ignored_group_base.id = "ignored-group-base".into();
    ignored_group_base.element = "ignored-group-element".into();
    ignored_group_base.object_id = 21;
    ignored_group_base.flags = 1;
    ignored_group_base.source_offset = 125;
    let mut ignored_group_element = group_element.clone();
    ignored_group_element.id = "ignored-group-element".into();
    ignored_group_element.ordinal = 4;
    ignored_group_element.object_id = 21;
    ignored_group_element.source_offset = 125;
    let ignored_group = DisplayJtGroupNodeData {
        id: "ignored-group-node".into(),
        base_node: "ignored-group-base".into(),
        object_id: 21,
        version: 1,
        child_object_ids: vec![9],
        family_data_byte_len: 0,
        family_data_sha256: "00".repeat(32),
        source_offset: 125,
    };
    let transform = DisplayJtGeometricTransformAttribute {
        id: "transform".into(),
        element: "scene-element".into(),
        object_id: 10,
        state_flags: 0,
        field_inhibit_flags: 0,
        stored_values_mask: 0xffff,
        matrix: [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 4.0, 0.0],
            [0.01, 0.02, 0.03, 1.0],
        ],
        source_offset: 121,
    };
    let node = DisplayJtTriStripShapeNode {
        id: "shape-node".into(),
        base_node: "base".into(),
        object_id: 9,
        reserved_bounds: [[0.0; 3]; 2],
        untransformed_bounds: [[0.0; 3]; 2],
        area: 0.0,
        vertex_count_range: [0, 0],
        node_count_range: [0, 0],
        polygon_count_range: [0, 0],
        memory_byte_len: 0,
        compression_level: 0.0,
        vertex_version: 1,
        vertex_bindings: 2,
        vertex_quantization_bits: 0,
        normal_quantization_factor: 0,
        texture_quantization_bits: 0,
        color_quantization_bits: 0,
        version_2_vertex_bindings: None,
        source_offset: 120,
    };
    let vertex_header = DisplayJtCompressedVertexRecordsHeader {
        id: "vertex-header".into(),
        element: "shape-element".into(),
        vertex_bindings: 0x15a,
        vertex_quantization_bits: 0,
        normal_quantization_factor: 0,
        texture_quantization_bits: 0,
        color_quantization_bits: 0,
        topological_vertex_count: 3,
        vertex_attribute_count: 3,
        compressed_arrays_byte_len: 0,
        compressed_arrays_sha256: "00".repeat(32),
        source_offset: 80,
    };
    let normals = DisplayJtVertexNormals {
        id: "normals".into(),
        vertex_records_header: "vertex-header".into(),
        normals: vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        normal_hash: 0,
        byte_len: 4,
        source_offset: 94,
    };
    let colors = DisplayJtVertexColors {
        id: "colors".into(),
        vertex_records_header: "vertex-header".into(),
        colors: vec![
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 0.5],
            [0.0, 0.0, 1.0, 0.25],
        ],
        color_hash: 0,
        byte_len: 4,
        source_offset: 98,
    };
    let texture_coordinates = DisplayJtVertexTextureCoordinates {
        id: "texture".into(),
        vertex_records_header: "vertex-header".into(),
        channel: 0,
        values: vec![vec![0.0, 0.0], vec![1.0, 0.0], vec![0.0, 1.0]],
        texture_coordinate_hash: 0,
        byte_len: 4,
        source_offset: 102,
    };
    let vertex_flags = DisplayJtVertexFlags {
        id: "flags".into(),
        vertex_records_header: "vertex-header".into(),
        values: vec![0, 1, 0],
        byte_len: 4,
        source_offset: 106,
    };

    let tessellations =
        crate::native::display_jt_tessellations(&crate::native::DisplayJtTessellationInputs {
            meshes: &[mesh],
            coordinates: &[coordinates],
            normals: &[normals],
            colors: &[colors],
            texture_coordinates: &[texture_coordinates],
            vertex_flags: &[vertex_flags],
            vertex_headers: &[vertex_header],
            coordinate_headers: &[header],
            shape_elements: &[shape_element],
            bindings: &[binding],
            shape_nodes: &[node],
            base_nodes: &[
                base,
                instance_base,
                second_instance_base,
                group_base,
                ignored_group_base,
            ],
            group_nodes: &[group, ignored_group],
            instance_nodes: &[instance, second_instance],
            transforms: &[transform],
            compressed_elements: &[
                compressed,
                instance_element,
                second_instance_element,
                group_element,
                ignored_group_element,
            ],
        })
        .expect("complete scene binding");
    assert_eq!(tessellations.len(), 2);
    assert!((tessellations[0].0.vertices[1].x - 12.0).abs() < 1e-6);
    assert!((tessellations[0].0.vertices[2].y - 26.0).abs() < 1e-6);
    assert_eq!(tessellations[0].0.triangles, vec![[0, 1, 2]]);
    assert_eq!(
        tessellations[0].0.normals[1],
        cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
    );
    assert_eq!(
        tessellations[0].0.source_object.as_ref().unwrap().object_id,
        "shape-node"
    );
    assert_eq!(
        tessellations[0]
            .0
            .source_object
            .as_ref()
            .unwrap()
            .instance_path,
        ["instance-node"]
    );
    assert_eq!(
        tessellations[1]
            .0
            .source_object
            .as_ref()
            .unwrap()
            .instance_path,
        ["second-instance-node"]
    );
    assert_eq!(tessellations[0].0.channels.len(), 3);
    assert_eq!(tessellations[0].0.channels[0].kind, 0x4e58_0001);
    assert_eq!(tessellations[0].0.channels[0].item_size, 16);
    assert_eq!(tessellations[0].0.channels[0].flags, 1);
    assert_eq!(tessellations[0].0.channels[0].count, 3);
    assert_eq!(
        &tessellations[0].0.channels[0].data[16..32],
        &[
            0.0_f32.to_le_bytes(),
            1.0_f32.to_le_bytes(),
            0.0_f32.to_le_bytes(),
            0.5_f32.to_le_bytes(),
        ]
        .concat()
    );
    assert_eq!(tessellations[0].0.channels[1].kind, 0x4e58_0100);
    assert_eq!(tessellations[0].0.channels[1].item_size, 8);
    assert_eq!(tessellations[0].0.channels[1].flags, 0x100);
    assert_eq!(tessellations[0].0.channels[1].count, 3);
    assert_eq!(
        &tessellations[0].0.channels[1].data[16..24],
        &[0.0_f32.to_le_bytes(), 1.0_f32.to_le_bytes()].concat()
    );
    assert_eq!(tessellations[0].0.channels[2].kind, 0x4e58_0002);
    assert_eq!(tessellations[0].0.channels[2].item_size, 4);
    assert_eq!(tessellations[0].0.channels[2].count, 3);
    assert_eq!(
        tessellations[0].0.channels[2].data,
        [
            0_u32.to_le_bytes(),
            1_u32.to_le_bytes(),
            0_u32.to_le_bytes(),
        ]
        .concat()
    );
    assert_eq!(tessellations[0].1, 120);
}

#[test]
fn jt_deering_normal_applies_sextant_octant_and_code_bounds() {
    let normal = crate::jt::deering_normal(1, 7, 8191, 0, 13).unwrap();
    assert!(normal[0].abs() < 1e-3);
    assert!(normal[1].abs() < 1e-6);
    assert!((normal[2] - 1.0).abs() < 1e-6);
    assert!(crate::jt::deering_normal(6, 7, 0, 0, 13).is_none());
    assert!(crate::jt::deering_normal(0, 8, 0, 0, 13).is_none());
    assert!(crate::jt::deering_normal(0, 7, 8192, 0, 13).is_none());
}

#[test]
fn jt_quantized_texture_coordinates_decode_component_major_lag1_codes() {
    let mut code = Vec::new();
    let mut push = |value: u32, width: u8| {
        code.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
    };
    push(0, 1);
    push(0, 6);
    push(3, 6);
    push(3, 3);
    for value in 0..4 {
        push(value, 2);
    }
    let mut word = 0u32;
    for bit in &code {
        word = (word << 1) | u32::from(*bit);
    }
    word <<= 32 - code.len();
    let mut packet = 4_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(&(code.len() as u32).to_le_bytes());
    packet.extend_from_slice(&word.to_le_bytes());

    let mut array = 4_u32.to_le_bytes().to_vec();
    array.extend_from_slice(&[2, 2]);
    for _ in 0..2 {
        array.extend_from_slice(&0_f32.to_le_bytes());
        array.extend_from_slice(&3_f32.to_le_bytes());
        array.push(2);
    }
    array.extend_from_slice(&packet);
    array.extend_from_slice(&packet);
    array.extend_from_slice(&0x8765_4321_u32.to_le_bytes());

    let (values, hash, consumed) =
        crate::jt::decode_vertex_texture_coordinates(&array, 4, 2).unwrap();
    assert_eq!(hash, 0x8765_4321);
    assert_eq!(consumed, array.len());
    assert_eq!(values[0], vec![-0.5, -0.5]);
    assert_eq!(values[3], vec![2.5, 2.5]);
}

#[test]
fn jt_quantized_colors_decode_rgb_and_hsv_quantizers() {
    let mut code = Vec::new();
    let mut push = |value: u32, width: u8| {
        code.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
    };
    push(0, 1);
    push(0, 6);
    push(3, 6);
    push(3, 3);
    for value in 0..4 {
        push(value, 2);
    }
    let mut word = 0u32;
    for bit in &code {
        word = (word << 1) | u32::from(*bit);
    }
    word <<= 32 - code.len();
    let mut packet = 4_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(&(code.len() as u32).to_le_bytes());
    packet.extend_from_slice(&word.to_le_bytes());

    let mut rgb = 4_u32.to_le_bytes().to_vec();
    rgb.extend_from_slice(&[3, 2, 0]);
    for _ in 0..4 {
        rgb.extend_from_slice(&0_f32.to_le_bytes());
        rgb.extend_from_slice(&3_f32.to_le_bytes());
        rgb.push(2);
    }
    for _ in 0..4 {
        rgb.extend_from_slice(&packet);
    }
    rgb.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
    let (colors, hash, consumed) = crate::jt::decode_vertex_colors(&rgb, 4, 2).unwrap();
    assert_eq!(hash, 0x1234_5678);
    assert_eq!(consumed, rgb.len());
    assert_eq!(colors[0], [-0.5; 4]);
    assert_eq!(colors[3], [2.5; 4]);

    let mut hsv = 4_u32.to_le_bytes().to_vec();
    hsv.extend_from_slice(&[4, 2, 1, 2, 2, 2, 2]);
    for _ in 0..4 {
        hsv.extend_from_slice(&packet);
    }
    hsv.extend_from_slice(&0x8765_4321_u32.to_le_bytes());
    let (colors, hash, consumed) = crate::jt::decode_vertex_colors(&hsv, 4, 2).unwrap();
    assert_eq!(hash, 0x8765_4321);
    assert_eq!(consumed, hsv.len());
    assert!(colors
        .iter()
        .flatten()
        .all(|component| component.is_finite()));
    assert!((colors[1][0] - 1.0 / 6.0).abs() < 1e-6);
    assert!((colors[1][1] - 1.0 / 6.0).abs() < 1e-6);
    assert!((colors[1][2] - 5.0 / 36.0).abs() < 1e-6);
    assert!((colors[1][3] - 1.0 / 6.0).abs() < 1e-6);
}

#[test]
fn jt_vertex_flags_require_a_complete_binary_value_packet() {
    let mut bits = vec![0];
    let mut field = |value: u32, width: u8| {
        bits.extend((0..width).rev().map(|shift| ((value >> shift) & 1) as u8));
    };
    field(1, 6);
    field(2, 6);
    field(0, 1);
    field(1, 2);
    field(0, 1);
    field(1, 1);
    field(0, 1);
    let mut word = 0u32;
    for bit in &bits {
        word = (word << 1) | u32::from(*bit);
    }
    word <<= 32 - bits.len();
    let mut packet = 3_u32.to_le_bytes().to_vec();
    packet.push(1);
    packet.extend_from_slice(&(bits.len() as u32).to_le_bytes());
    packet.extend_from_slice(&word.to_le_bytes());
    let mut array = 3_u32.to_le_bytes().to_vec();
    array.extend_from_slice(&packet);

    assert_eq!(
        crate::jt::decode_vertex_flags(&array, 3),
        Some((vec![0, 1, 0], array.len()))
    );
    assert!(crate::jt::decode_vertex_flags(&array, 2).is_none());
    let last = array.len() - 1;
    array[last] |= 1;
    assert!(crate::jt::decode_vertex_flags(&array, 3).is_none());
}

#[test]
fn jt9_topology_bounds_variable_high_degree_lane_count() {
    fn representation(high_degree_lanes: usize, topological_vertices: u32) -> Vec<u8> {
        let mut bytes = vec![0; (21 + high_degree_lanes + 2) * 4];
        bytes.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
        bytes.extend_from_slice(&10_u64.to_le_bytes());
        bytes.extend_from_slice(&[24, 13, 16, 8]);
        bytes.extend_from_slice(&topological_vertices.to_le_bytes());
        if topological_vertices != 0 {
            bytes.extend_from_slice(&(topological_vertices + 1).to_le_bytes());
        }
        bytes
    }

    let empty = representation(1, 0);
    assert_eq!(
        crate::native::jt9_topology_high_degree_lane_count(&empty, 10),
        Some(1)
    );
    let populated = representation(13, 20);
    assert_eq!(
        crate::native::jt9_topology_high_degree_lane_count(&populated, 10),
        Some(13)
    );
    assert_eq!(
        crate::native::jt9_topology_high_degree_lane_count(&populated, 11),
        None
    );
}

#[test]
fn jt9_topology_packets_retain_decoded_primal_values() {
    use crate::native::{display_jt_topology_packet_sequences, DisplayJtShapeLodElement};

    let mut representation = vec![0; 24 * 4];
    representation.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
    representation.extend_from_slice(&10_u64.to_le_bytes());
    representation.extend_from_slice(&[24, 13, 16, 8]);
    representation.extend_from_slice(&0_u32.to_le_bytes());

    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&10_u64.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&representation);
    let source_offset = 64_u64;
    let mut data = vec![0; source_offset as usize + 25];
    data.extend_from_slice(&body);
    let container = crate::container::Container {
        data,
        version: 1,
        file_tag: 0,
        footer_offset: 0,
        entries: Vec::new(),
    };
    let elements = [DisplayJtShapeLodElement {
        id: "shape-lod".into(),
        segment: "segment".into(),
        ordinal: 0,
        object_type_id: vec![
            0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb,
            0x59, 0x97,
        ],
        object_base_type: 4,
        object_id: 1,
        body_byte_len: body.len() as u32,
        body_sha256: String::new(),
        source_offset,
    }];

    let (sequences, _, _) = display_jt_topology_packet_sequences(&container, &elements);
    assert_eq!(sequences.len(), 1);
    assert_eq!(sequences[0].packets.len(), 24);
    assert!(sequences[0]
        .packets
        .iter()
        .all(|packet| packet.values == Some(Vec::new())));
}

#[test]
fn display_jt_base_node_body_bounds_ordered_attribute_ids() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0x20_u32.to_le_bytes());
    body.extend_from_slice(&2_u32.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&9_u32.to_le_bytes());
    body.extend_from_slice(&[4, 3, 2, 1]);
    let (version, flags, attributes, family) =
        crate::native::parse_jt_base_node_body(&body, 9).unwrap();
    assert_eq!(version, 1);
    assert_eq!(flags, 0x20);
    assert_eq!(attributes, [7, 9]);
    assert_eq!(family, [4, 3, 2, 1]);

    body.truncate(17);
    assert!(crate::native::parse_jt_base_node_body(&body, 9).is_none());

    let mut modern = vec![2];
    modern.extend_from_slice(&0x40_u32.to_le_bytes());
    modern.extend_from_slice(&1_u32.to_le_bytes());
    modern.extend_from_slice(&11_u32.to_le_bytes());
    modern.push(0xaa);
    let (version, flags, attributes, family) =
        crate::native::parse_jt_base_node_body(&modern, 10).unwrap();
    assert_eq!((version, flags), (2, 0x40));
    assert_eq!(attributes, [11]);
    assert_eq!(family, [0xaa]);
}

#[test]
fn display_jt9_instance_node_requires_one_exact_child_reference() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0x20_u32.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&9_u32.to_le_bytes());

    assert_eq!(
        crate::native::parse_jt9_instance_node_body(&body),
        Some((1, 9))
    );
    body.push(0);
    assert!(crate::native::parse_jt9_instance_node_body(&body).is_none());
    body.pop();
    body[14..16].copy_from_slice(&2_u16.to_le_bytes());
    assert!(crate::native::parse_jt9_instance_node_body(&body).is_none());
}

#[test]
fn display_jt9_group_node_bounds_ordered_children_and_family_tail() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&2_u32.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&9_u32.to_le_bytes());
    body.extend_from_slice(&[4, 3, 2, 1]);

    let (version, children, family) = crate::native::parse_jt9_group_node_body(&body).unwrap();
    assert_eq!(version, 1);
    assert_eq!(children, [7, 9]);
    assert_eq!(family, [4, 3, 2, 1]);
    body.truncate(body.len() - 5);
    assert!(crate::native::parse_jt9_group_node_body(&body).is_none());
}

#[test]
fn display_jt9_tri_strip_shape_node_requires_exact_shape_data() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0x20_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    for value in [0.0_f32, 1.0, 2.0, 3.0, 4.0, 5.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    for value in [-3.0_f32, -2.0, -1.0, 0.0, 1.0, 2.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    body.extend_from_slice(&6.0_f32.to_le_bytes());
    for value in [7_i32, 8, 9, 10, 11, 12] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    body.extend_from_slice(&4096_u32.to_le_bytes());
    body.extend_from_slice(&0.75_f32.to_le_bytes());
    body.extend_from_slice(&2_u16.to_le_bytes());
    body.extend_from_slice(&0x102_u64.to_le_bytes());
    body.extend_from_slice(&[24, 13, 16, 8]);
    body.extend_from_slice(&0x304_u64.to_le_bytes());

    let node = crate::native::parse_jt9_tri_strip_shape_node_body(&body).unwrap();
    assert_eq!(node.reserved_bounds, [[0.0, 1.0, 2.0], [3.0, 4.0, 5.0]]);
    assert_eq!(
        node.untransformed_bounds,
        [[-3.0, -2.0, -1.0], [0.0, 1.0, 2.0]]
    );
    assert_eq!(node.area, 6.0);
    assert_eq!(node.vertex_count_range, [7, 8]);
    assert_eq!(node.node_count_range, [9, 10]);
    assert_eq!(node.polygon_count_range, [11, 12]);
    assert_eq!(node.memory_byte_len, 4096);
    assert_eq!(node.compression_level, 0.75);
    assert_eq!(node.vertex_version, 2);
    assert_eq!(node.vertex_bindings, 0x102);
    assert_eq!(node.vertex_quantization_bits, 24);
    assert_eq!(node.normal_quantization_factor, 13);
    assert_eq!(node.texture_quantization_bits, 16);
    assert_eq!(node.color_quantization_bits, 8);
    assert_eq!(node.version_2_vertex_bindings, Some(0x304));

    let mut malformed = body.clone();
    malformed[60..64].copy_from_slice(&(-1.0_f32).to_le_bytes());
    assert!(crate::native::parse_jt9_tri_strip_shape_node_body(&malformed).is_none());
    let mut malformed = body.clone();
    malformed[109] = 25;
    assert!(crate::native::parse_jt9_tri_strip_shape_node_body(&malformed).is_none());
    body.truncate(body.len() - 8);
    assert!(crate::native::parse_jt9_tri_strip_shape_node_body(&body).is_none());
}

#[test]
fn display_jt9_geometric_transform_reconstructs_sparse_affine_matrix() {
    let mut body = 1_u16.to_le_bytes().to_vec();
    body.push(0x08);
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0x000e_u16.to_le_bytes());
    for value in [1.25_f32, -2.5, 4.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    let (state, inhibit, mask, matrix) =
        crate::native::parse_jt9_geometric_transform_body(&body).unwrap();
    assert_eq!(state, 0x08);
    assert_eq!(inhibit, 0);
    assert_eq!(mask, 0x000e);
    assert_eq!(matrix[0], [1.0, 0.0, 0.0, 0.0]);
    assert_eq!(matrix[3], [1.25, -2.5, 4.0, 1.0]);

    body[2] = 0x10;
    assert!(crate::native::parse_jt9_geometric_transform_body(&body).is_none());

    let mut shear = 1_u16.to_le_bytes().to_vec();
    shear.push(0);
    shear.extend_from_slice(&0_u32.to_le_bytes());
    shear.extend_from_slice(&1_u16.to_le_bytes());
    shear.extend_from_slice(&0x4800_u16.to_le_bytes());
    shear.extend_from_slice(&0.5_f32.to_le_bytes());
    shear.extend_from_slice(&0.5_f32.to_le_bytes());
    assert!(crate::native::parse_jt9_geometric_transform_body(&shear).is_none());
}

#[test]
fn display_jt9_partition_node_requires_complete_bounds_and_ranges() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&2_u32.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&u16::from(b'x').to_le_bytes());
    for value in [0.0_f32, 1.0, 2.0, 3.0, 4.0, 5.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    body.extend_from_slice(&6.0_f32.to_le_bytes());
    for value in [1_i32, 2, 3, 4, 5, 6] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    for value in [-3.0_f32, -2.0, -1.0, 0.0, 1.0, 2.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    let node = crate::native::parse_jt9_partition_node_body(&body).unwrap();
    assert_eq!(node.group_version, 1);
    assert_eq!(node.child_object_ids, [2]);
    assert_eq!(node.file_name, "x");
    assert_eq!(node.transformed_bounds, [[0.0, 1.0, 2.0], [3.0, 4.0, 5.0]]);
    assert_eq!(node.area, 6.0);
    assert_eq!(node.vertex_count_range, [1, 2]);
    assert_eq!(node.node_count_range, [3, 4]);
    assert_eq!(node.polygon_count_range, [5, 6]);
    assert_eq!(
        node.untransformed_bounds,
        Some([[-3.0, -2.0, -1.0], [0.0, 1.0, 2.0]])
    );
    assert!(node.reserved_bounds.is_none());

    body.pop();
    assert!(crate::native::parse_jt9_partition_node_body(&body).is_none());
}

#[test]
fn display_jt9_range_lod_requires_ordered_finite_limits() {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&2_u32.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&9_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&0.25_f32.to_le_bytes());
    body.extend_from_slice(&(-2_i32).to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&2_u32.to_le_bytes());
    body.extend_from_slice(&10.0_f32.to_le_bytes());
    body.extend_from_slice(&20.0_f32.to_le_bytes());
    for value in [1.0_f32, 2.0, 3.0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    let node = crate::native::parse_jt9_range_lod_node_body(&body).unwrap();
    assert_eq!(node.group_version, 1);
    assert_eq!(node.child_object_ids, [7, 9]);
    assert_eq!(node.lod_version, 1);
    assert_eq!(node.reserved_values, [0.25]);
    assert_eq!(node.reserved_value, -2);
    assert_eq!(node.range_version, 1);
    assert_eq!(node.range_limits, [10.0, 20.0]);
    assert_eq!(node.center, [1.0, 2.0, 3.0]);

    let range_offset = body.len() - 20;
    body[range_offset..range_offset + 4].copy_from_slice(&5.0_f32.to_le_bytes());
    body[range_offset + 4..range_offset + 8].copy_from_slice(&4.0_f32.to_le_bytes());
    assert!(crate::native::parse_jt9_range_lod_node_body(&body).is_none());
}

fn be_f64(v: f64) -> [u8; 8] {
    v.to_be_bytes()
}

fn segment_index_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [7u32, 9, 11, 1, 1, 28] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]);
    payload
}

fn segment_stream_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    payload.extend_from_slice(&0x8000_0000u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(b"PS\0\0 (deltas) SCH_test segment stream payload with more than sixty-four inflated bytes........")
        .unwrap();
    payload.extend_from_slice(&encoder.finish().unwrap());
    payload
}

fn segment_body_binding_payload(stream_kind: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [7u32, 9, 11, 1, 1, 48, 64, 0, 94, 150, 19, 0] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(64, 0);
    payload.extend_from_slice(&0x8000_0000u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(
            format!(
                "PS\0\0 ({stream_kind}) SCH_test segment body binding payload with more than sixty-four inflated bytes........"
            )
            .as_bytes(),
        )
        .unwrap();
    payload.extend_from_slice(&encoder.finish().unwrap());
    payload
}

fn segment_extended_wrapper_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [7u32, 9, 11, 1, 1, 48, 64, 0, 94, 150, 19, 0] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(64, 0);
    payload.extend_from_slice(&0xc000_0005u32.to_le_bytes());
    payload.resize(64 + 38, 0);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(b"PS\0\0 (partition) SCH_test extended wrapper payload with more than sixty-four inflated bytes........")
        .unwrap();
    payload.extend_from_slice(&encoder.finish().unwrap());
    payload
}

fn segment_om_payload(separated: bool) -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    if separated {
        payload.extend_from_slice(&[0xc0, 0xd1, 0xf1, 0xed]);
    }
    payload.extend_from_slice(&size_framed_om_section());
    payload
}

fn segment_om_record_area_payload() -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    payload.extend_from_slice(&size_framed_om_section_with_record_area());
    payload
}

fn multi_section_feature_history_payload() -> Vec<u8> {
    let mut early = size_framed_om_section_with_record_area();
    let name = early
        .windows(b"UNITE".len())
        .position(|window| window == b"UNITE")
        .expect("operation label");
    early[name..name + b"BLOCK".len()].copy_from_slice(b"BLOCK");
    let late = size_framed_om_section_with_record_area();
    let index_byte_len = 36_u32;
    let early_offset = index_byte_len;
    let late_offset = early_offset + early.len() as u32;
    let mut payload = Vec::new();
    for word in [
        late_offset,
        early_offset,
        11,
        1,
        1,
        index_byte_len,
        early_offset,
        9,
        11,
    ] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend_from_slice(&early);
    payload.extend_from_slice(&late);
    payload
}

fn segment_om_record_area_with_input_store_payload() -> Vec<u8> {
    let mut payload = segment_om_record_area_payload();
    let mut store = offset_only_indexed_om_section();
    let base = payload.len() as u32;
    let index_start = 8 + 1 + b"UGS::ModlFeature".len() + 1;
    for index in 0..4 {
        let at = index_start + index * 4;
        let value = u32::from_le_bytes(store[at..at + 4].try_into().unwrap());
        store[at..at + 4].copy_from_slice(&(value + base).to_le_bytes());
    }
    payload.extend_from_slice(&store);
    payload
}

/// Append one feature-history operation record (label header + object-index
/// slots + typed payload) to a record area under construction.
fn push_feature_operation(bytes: &mut Vec<u8>, object_indices: &[u8], label: &str, payload: &[u8]) {
    const HEADER: &[u8] = &[
        0x80, 0xcd, 0x01, 0x04, 0x01, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b, 0xff, 0xff,
    ];
    bytes.extend_from_slice(HEADER);
    bytes.extend_from_slice(object_indices);
    bytes.push(0x03);
    bytes.push((label.len() + 2) as u8);
    bytes.extend_from_slice(label.as_bytes());
    bytes.push(0x00);
    bytes.extend_from_slice(payload);
}

/// A feature-history (`UGS::FEATURE_RECORD`) size-framed OM section whose record
/// area packs the supplied operations. `operations` is `(object_index_slots,
/// label, typed_payload)`.
fn composed_feature_history_section(operations: &[(&[u8], &str, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = size_framed_om_section();
    let record_area = bytes.len() + 20;
    bytes.extend_from_slice(&(record_area as u32).to_le_bytes());
    bytes.resize(record_area, 0);
    bytes.extend_from_slice(&13u32.to_le_bytes());
    bytes.extend_from_slice(&14u32.to_le_bytes());
    bytes.extend_from_slice(&44u32.to_le_bytes());
    bytes.extend_from_slice(b"\x05\x01\x0eNX 2027.3102\0");
    for (slots, label, payload) in operations {
        push_feature_operation(&mut bytes, slots, label, payload);
    }
    let payload_len = (bytes.len() - 16) as u32;
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());
    bytes
}

/// An offset-store indexed OM section carrying `records` as its object-id-less
/// data blocks. The single product record lives in the control block (index 0)
/// so the section validates; `records[i]` resolves to `block#{i + 1}`.
fn composed_offset_store(records: &[&[u8]]) -> Vec<u8> {
    let mut bytes = vec![0xaa; 8];
    let class_name = b"UGS::ModlFeature";
    bytes.push((class_name.len() + 1) as u8);
    bytes.extend_from_slice(class_name);
    bytes.push(0x81);
    let index_start = bytes.len();
    let offset_count = records.len() + 2;
    bytes.resize(index_start + offset_count * 4, 0);
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    let mut offsets = Vec::with_capacity(offset_count);
    offsets.push(bytes.len());
    bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    for record in records {
        offsets.push(bytes.len());
        bytes.extend_from_slice(record);
    }
    offsets.push(bytes.len());
    for (index, offset) in offsets.iter().enumerate() {
        bytes[index_start + index * 4..index_start + index * 4 + 4]
            .copy_from_slice(&(*offset as u32).to_le_bytes());
    }
    bytes
}

/// Compose a UG_PART payload: segment-index header, one feature-history section
/// with `operations`, and one appended offset store carrying `store_records`.
fn composed_feature_history_payload(
    operations: &[(&[u8], &str, Vec<u8>)],
    store_records: &[&[u8]],
) -> Vec<u8> {
    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    payload.extend_from_slice(&composed_feature_history_section(operations));

    let mut store = composed_offset_store(store_records);
    let base = payload.len() as u32;
    let index_start = 8 + 1 + b"UGS::ModlFeature".len() + 1;
    let offset_count = store_records.len() + 2;
    for index in 0..offset_count {
        let at = index_start + index * 4;
        let value = u32::from_le_bytes(store[at..at + 4].try_into().unwrap());
        store[at..at + 4].copy_from_slice(&(value + base).to_le_bytes());
    }
    payload.extend_from_slice(&store);
    payload
}

type ComposedInputs = (
    Vec<(&'static [u8], &'static str, Vec<u8>)>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
    Vec<u8>,
);

/// A 31-character lowercase-hex identity (no `f`, so no `0x66` name markers)
/// shared by the datum-CSYS descriptor in `block3` and the datum-plane
/// descriptor in `block5`, joining them through `datum_plane_csys_identity_uses`.
const COMPOSED_DESCRIPTOR_IDENTITY: &[u8] = b"0123456789abcde0123456789abcde0";

/// Build the operation list and four offset-store data blocks for the composed
/// feature-history fixture.
///
/// - block1+block2 form a two-block offset-store named point `Point7`;
/// - block3+block4 carry rich sketch geometry (named points, scalar fields,
///   coordinate and fixed pairs, and datum-CSYS pair discriminators).
///
/// Operations: `SKETCH` referencing the named point (object indices 1,2),
/// `SKETCH` referencing the geometry (3,4), `DATUM_CSYS` (eight refs to 1) and
/// `DATUM_PLANE`.
fn composed_feature_history_inputs() -> ComposedInputs {
    let sketch_named = vec![
        0x01, 0x00, 0x01, 0x02, 0xf0, 0x01, 0x00, 0x00, 0xf0, 0x02, 0x01, 0x00, 0x00, 0x00,
    ];
    let sketch_geometry = vec![
        0x01, 0x00, 0x01, 0x02, 0xf0, 0x03, 0x00, 0x00, 0xf0, 0x04, 0x01, 0x00, 0x00, 0x00,
    ];
    let mut datum_csys = vec![
        0x13, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ];
    for _ in 0..8 {
        datum_csys.extend_from_slice(&[0xf0, 0x03]);
    }
    datum_csys.extend_from_slice(&[0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]);
    // Single-reference datum-plane branch: descriptor index 5 (block5, a 40-byte
    // descriptor) and object index 3 (block3, the object payload).
    let datum_plane = vec![
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x02, 0x23, 0x01, 0x02, 0x05, 0x01, 0xf0, 0x03, 0x00,
        0x14, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00,
    ];

    // Additional container-level operations whose typed payloads each populate a
    // reference/header/lane family directly (mirrors the per-parser white-box
    // fixtures). Object indices in these payloads are intentionally large and do
    // not resolve, so only the leading reference/header arenas populate.
    let point = b"\x72\x00\x00\x01\x00\x00\x00\xf1\x1c\x8f\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x0d\x01\x02\x01\x00\x00\x00\x89\x02\x01\x01\x01\x00\xa5\x57\x95\x01\x00\x00\xff\x02\xc0\x1f\xff\xfd\x01\x00\x00\x01\x01\x01\x03\x02\x01\x01\x01\x00\x00\x00\x00\x00\xaa".to_vec();
    let draft = {
        let prefix = b"\x67\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\xff\xff\xff\xff\xff\xff\xff\xff\x01\x03\x80\x94\x82\x49".as_slice();
        let graph = b"\x01\x02\xf1\x1b\x7c\x01\x02\xf1\x1b\x7d\x68\x2f\x70\x62\x4d\xd2\xf1\xa9\xfc\x03\x50\x44\x00\x00\x01\x46\x8a\x2a\x01\xa3\x60\x10\x01\x01\x01\x04\x02\x01\x02\x01\x00\x00\x00\x00\x01\xf1\x1b\x7e\xff\x00\x00\x00\xf1\x1b\x7f\xff".as_slice();
        let terminal =
            b"\x81\x5e\x80\xb8\x01\x03\x02\x01\x02\x01\x01\x01\x00\x00\x00\x29\x29\x0c\x00"
                .as_slice();
        [prefix, graph, terminal].concat()
    };
    let surface = b"\x3f\x00\x00\x01\x00\xf1\x02\x46\xf1\x02\x47\xf1\x02\x48\x01\x09\x03\x03\x04\x05\x02\x01\x01\x01\x01\x09\xf1\x02\x49\xf1\x02\x4a\xf1\x02\x4b\xf1\x02\x4c\xf1\x02\x4d\xf1\x02\x4e\xf1\x02\x4f\xf1\x02\x50\x00\x03\x03\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xf1\x02\x56\xf1\x02\x57\xf1\x02\x58\x01\x01\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x01\x02".to_vec();
    let pattern_refs = b"\x44\x45\x00\xff\xff\xf1\x03\x21\x01\x02\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x01\x02".to_vec();
    let pattern_lane = b"\xaa\x01\x03\x60\x01\x00\x00\x50\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\x60\x01\x00\x00\xd0\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x9f\xfe\x01\x02\x00\x00\xff\x00\x00\x5f\x00\x00\x01".to_vec();
    let extrude_profile = b"\x01\x02\x16\x01\x03\xf0\xff\xf1\x01\x00\x01\x03\x79\xaa\x01\x03\xf0\xff\xf1\x01\x00\x00\x00".to_vec();
    let extrude_header =
        b"\x0f\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x2f\xa3\x74\xbc\x6a\x7e\xf9\xdb"
            .to_vec();
    let extrude_footer = b"\x01\x01\x02\x81\x5f\x80\xab\x01\x03\x02\x01\x01\x02\x01\x01\x00\x00\x00\x29\x29\x05\x80\xff\x00".to_vec();
    let block = {
        let mut payload = vec![0x26u8, 0, 0, 1, 0, 0];
        for value in 1..=18u8 {
            payload.extend([0xf0, value]);
        }
        payload.extend([0x01, 0xf1, 0x01, 0x00]);
        payload.extend([0xff; 11]);
        payload.extend([0; 4]);
        payload
    };
    let projected_curve =
        b"\0\x01\x02\xf1\x02\xc8\xf1\x02\xc9\x80\x57\x00\x02\x01\xf1\x02\xca\xff\x01\x02\x02\x7d\0"
            .to_vec();
    // SIMPLE HOLE: two identical scalar runs, each followed by two block-reference
    // tokens, then a canonical `Hole_...` template string.
    let simple_hole = {
        let mut payload = Vec::new();
        payload.extend_from_slice(&shifted_f64_bytes(508.0));
        payload.extend_from_slice(&shifted_f64_bytes(38.1));
        payload.extend_from_slice(&[0xf0, 0x03, 0xf0, 0x04]);
        payload.extend_from_slice(&shifted_f64_bytes(508.0));
        payload.extend_from_slice(&shifted_f64_bytes(38.1));
        payload.extend_from_slice(&[0xf0, 0x03, 0xf0, 0x04]);
        let template = b"Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer";
        payload.extend_from_slice(&[0x04, (template.len() + 2) as u8]);
        payload.extend_from_slice(template);
        payload.push(0x00);
        payload
    };

    let operations: Vec<(&'static [u8], &'static str, Vec<u8>)> = vec![
        (&[1, 0xff, 0xff, 0xff], "SKETCH", sketch_named),
        (&[3, 0xff, 0xff, 0xff], "SKETCH", sketch_geometry),
        (&[3, 0xff, 0xff, 0xff], "DATUM_CSYS", datum_csys),
        (&[3, 0xff, 0xff, 0xff], "DATUM_PLANE", datum_plane),
        (&[3, 0xff, 0xff, 0xff], "POINT", point),
        (&[3, 0xff, 0xff, 0xff], "DRAFT", draft),
        (&[3, 0xff, 0xff, 0xff], "SKIN", surface),
        (&[3, 0xff, 0xff, 0xff], "Geometry Instance", pattern_refs),
        (&[3, 0xff, 0xff, 0xff], "Pattern Feature", pattern_lane),
        (&[3, 0xff, 0xff, 0xff], "EXTRUDE", extrude_profile),
        (&[3, 0xff, 0xff, 0xff], "EXTRUDE", extrude_header),
        (&[3, 0xff, 0xff, 0xff], "EXTRUDE", extrude_footer),
        (&[3, 0xff, 0xff, 0xff], "BLOCK", block),
        (&[3, 0xff, 0xff, 0xff], "CPROJ", projected_curve),
        (&[3, 0xff, 0xff, 0xff], "SIMPLE HOLE", simple_hole),
    ];

    // Two-block offset-store named point `Point7` (leading name + scalar in
    // block1, the second scalar in block2).
    let mut block1: Vec<u8> = Vec::new();
    block1.extend_from_slice(&[0x03, 0x08]);
    block1.extend_from_slice(b"Point7");
    block1.push(0x00);
    block1.extend_from_slice(&[
        0x50, 0x59, 0x66, 0x58, 0x00, 0x30, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ]);
    let block2: Vec<u8> = vec![
        0x50, 0x59, 0x66, 0x59, 0x00, 0x31, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ];

    // Rich sketch geometry across block3 (payload) and block4 (terminal filler).
    let mut block3: Vec<u8> = Vec::new();
    // Point1: payload-leading name plus two PYf scalar fields.
    block3.extend_from_slice(&[0x03, 0x08]);
    block3.extend_from_slice(b"Point1");
    block3.push(0x00);
    block3.extend_from_slice(&[
        0x50, 0x59, 0x66, 0x58, 0x00, 0x30, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ]);
    block3.extend_from_slice(&[
        0x50, 0x59, 0x66, 0x59, 0x00, 0x31, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ]);
    // Point2: 66-form name plus one signed Q1.55 fixed pair (no scalars).
    block3.extend_from_slice(&[0x66, 0x32, 0x03, 0x08]);
    block3.extend_from_slice(b"Point2");
    block3.push(0x00);
    block3.extend_from_slice(&[
        0x04, 0xe0, 0x48, 0x0e, 0x02, 0x03, 0x80, 0x84, 0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x30, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]);
    // Point3: 66-form name closing Point2's named-record interval.
    block3.extend_from_slice(&[0x66, 0x32, 0x03, 0x08]);
    block3.extend_from_slice(b"Point3");
    block3.push(0x00);
    // Coordinate pair (object_payload_scalar_pairs SHORT discriminator).
    block3.extend_from_slice(&[
        0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
        0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0xc0, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ]);
    // datum_csys signed Q1.55 fixed pair (0b discriminator).
    block3.extend_from_slice(&[
        0x0b, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
        0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0xc0, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ]);
    // datum_plane object scalar pair (6d 00 f0 + coordinate discriminator).
    block3.extend_from_slice(&[0x6d, 0x00, 0xf0]);
    block3.extend_from_slice(&[
        0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
        0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0xc0, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ]);
    // datum_csys descriptor identity: a maximal 31-char hex run bounded by nulls.
    block3.push(0x00);
    block3.extend_from_slice(COMPOSED_DESCRIPTOR_IDENTITY);
    block3.push(0x00);
    let block4: Vec<u8> = vec![0x00];

    // block5: a 40-byte datum-plane descriptor block sharing the CSYS identity.
    let mut block5: Vec<u8> = Vec::new();
    block5.extend_from_slice(COMPOSED_DESCRIPTOR_IDENTITY); // hex identity (31)
    block5.extend_from_slice(b"?A"); // delimiter + form marker
    block5.push(0x03); // compact schema index
    block5.extend_from_slice(&[0xff, 0x02, 0x01]); // fixed separator
    block5.extend_from_slice(b"DPd"); // graphic label; pads block to 40 bytes
    debug_assert_eq!(block5.len(), 40);

    (operations, block1, block2, block3, block4, block5)
}

/// A `.prt` image whose single feature-history section and companion offset
/// store drive the feature-history arena families that no other golden reaches:
/// the complete sketch family (records, references, construction inputs and
/// payloads, coordinate/fixed pairs, scalars, names, named records, points,
/// fixed points, point groups, named-point/preceding/point uses, and the
/// datum-CSYS dependency), the datum-CSYS and datum-plane families (constructions,
/// payloads, pairs, scalars, descriptors, headers, block uses, identity uses),
/// plus the point/draft/surface/pattern/extrude/block reference and header lanes.
fn composed_feature_history_prt() -> Vec<u8> {
    let (operations, block1, block2, block3, block4, block5) = composed_feature_history_inputs();
    let store_records: Vec<&[u8]> = vec![&block1, &block2, &block3, &block4, &block5];
    let payload = composed_feature_history_payload(&operations, &store_records);
    prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
}

#[test]
fn nx_expression_parameter_references_preserve_formula_order() {
    assert_eq!(
        crate::native::expression_parameter_names(
            "max(p12, p3) + p12 + exp2 + p7_radius + p7_radius + p4bad + p5_"
        ),
        vec!["p12", "p3", "p12", "p7_radius", "p7_radius"]
    );
}

#[test]
fn nx_expression_graph_rejects_noncanonical_parameter_tokens() {
    let expression = |name: &str, formula: &str, value| crate::native::Expression {
        id: format!("nx:test:expression#{name}"),
        object_id: None,
        record: None,
        declaration: None,
        name: name.into(),
        parameter_index: None,
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: formula.into(),
        value,
        source_entry: "part".into(),
        source_table: "table".into(),
        source_offset: 0,
    };
    let mut expressions = vec![
        expression("p4", "3", Some(3.0)),
        expression("p5", "p4bad + 2", None),
        expression("p6", "p4_ + 2", None),
    ];

    crate::native::evaluate_expression_graphs(&mut expressions);

    assert_eq!(expressions[1].value, None);
    assert_eq!(expressions[2].value, None);
}

#[test]
fn nx_expression_graph_evaluates_exact_qualified_dependencies() {
    let expression = |name: &str, formula: &str, value| crate::native::Expression {
        id: format!("nx:test:expression#{name}"),
        object_id: None,
        record: None,
        declaration: None,
        name: name.into(),
        parameter_index: None,
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: formula.into(),
        value,
        source_entry: "part".into(),
        source_table: "table".into(),
        source_offset: 0,
    };
    let mut expressions = vec![
        expression("p7", "3", Some(3.0)),
        expression("p7_radius", "5", Some(5.0)),
        expression("p8", "p7_radius * 2", None),
        expression("p9", "p8 + p7", None),
    ];

    crate::native::evaluate_expression_graphs(&mut expressions);

    assert_eq!(expressions[2].value, Some(10.0));
    assert_eq!(expressions[3].value, Some(13.0));
}

#[test]
fn nx_expression_graph_substitutes_dependencies_as_atomic_operands() {
    let expression = |name: &str, formula: &str, value| crate::native::Expression {
        id: format!("nx:test:expression#{name}"),
        object_id: None,
        record: None,
        declaration: None,
        name: name.into(),
        parameter_index: None,
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: formula.into(),
        value,
        source_entry: "part".into(),
        source_table: "table".into(),
        source_offset: 0,
    };
    let mut expressions = vec![
        expression("p1", "-2", Some(-2.0)),
        expression("p2", "p1^2", None),
        expression("p3", "-p1^2", None),
    ];

    crate::native::evaluate_expression_graphs(&mut expressions);

    assert_eq!(expressions[1].value, Some(4.0));
    assert_eq!(expressions[2].value, Some(-4.0));
}

#[test]
fn nx_expression_graph_scopes_names_to_their_expression_table() {
    let expression =
        |id: &str, table: &str, name: &str, formula: &str, value| crate::native::Expression {
            id: id.into(),
            object_id: None,
            record: None,
            declaration: None,
            name: name.into(),
            parameter_index: None,
            qualifier: None,
            unit: crate::native::ExpressionUnit::Millimeter,
            expression: formula.into(),
            value,
            source_entry: "part".into(),
            source_table: table.into(),
            source_offset: 0,
        };
    let mut expressions = vec![
        expression("a-p2", "table-a", "p2", "5", Some(5.0)),
        expression("a-p3", "table-a", "p3", "p2 * 2", None),
        expression("b-p2", "table-b", "p2", "7", Some(7.0)),
        expression("b-p3", "table-b", "p3", "p2 * 2", None),
    ];

    crate::native::evaluate_expression_graphs(&mut expressions);

    assert_eq!(expressions[1].value, Some(10.0));
    assert_eq!(expressions[3].value, Some(14.0));
}

#[test]
fn nx_expression_graph_rejects_every_duplicate_name_in_one_table() {
    let expression =
        |id: &str, table: &str, name: &str, formula: &str, value| crate::native::Expression {
            id: id.into(),
            object_id: None,
            record: None,
            declaration: None,
            name: name.into(),
            parameter_index: None,
            qualifier: None,
            unit: crate::native::ExpressionUnit::Millimeter,
            expression: formula.into(),
            value,
            source_entry: "part".into(),
            source_table: table.into(),
            source_offset: 0,
        };
    let mut expressions = vec![
        expression("a-p1-first", "table-a", "p1", "3", Some(3.0)),
        expression("a-p1-second", "table-a", "p1", "5", Some(5.0)),
        expression("a-p2", "table-a", "p2", "p1 * 2", None),
        expression("b-p1", "table-b", "p1", "7", Some(7.0)),
        expression("b-p2", "table-b", "p2", "p1 * 2", None),
    ];

    crate::native::evaluate_expression_graphs(&mut expressions);

    assert_eq!(expressions[0].value, None);
    assert_eq!(expressions[1].value, None);
    assert_eq!(expressions[2].value, None);
    assert_eq!(expressions[3].value, Some(7.0));
    assert_eq!(expressions[4].value, Some(14.0));
}

#[test]
fn nx_formula_dependencies_resolve_to_section_parameters() {
    let expression = |key: u32,
                      name: &str,
                      index: u32,
                      qualifier: Option<&str>,
                      text: &str,
                      value: Option<f64>| crate::native::Expression {
        id: format!("nx:test:expression#{key}"),
        object_id: Some(key),
        record: None,
        declaration: None,
        name: name.into(),
        parameter_index: Some(index),
        qualifier: qualifier.map(str::to_string),
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: text.into(),
        value,
        source_entry: "/Root/UG_PART/UG_PART".into(),
        source_table: "table".into(),
        source_offset: u64::from(key),
    };
    let expressions = [
        expression(20, "p2", 2, None, "5", Some(5.0)),
        expression(21, "p2_radius", 2, Some("radius"), "7", Some(7.0)),
        expression(90, "p9", 9, None, "p2_radius * 2 + p2_radius", None),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::native::attach_expression_parameters(&mut ir, &expressions, &[], &[], &mut annotations);

    assert_eq!(ir.model.parameters[2].value, None);
    assert_eq!(
        ir.model.parameters[2].dependencies,
        vec![ir.model.parameters[1].id.clone()]
    );
}

#[test]
fn nx_formula_dependencies_reject_ambiguous_parameter_names() {
    let expression = |key: u32, name: &str, text: &str| crate::native::Expression {
        id: format!("nx:test:expression#{key}"),
        object_id: Some(key),
        record: None,
        declaration: None,
        name: name.into(),
        parameter_index: Some(key),
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: text.into(),
        value: None,
        source_entry: "/Root/UG_PART/UG_PART".into(),
        source_table: "table".into(),
        source_offset: u64::from(key),
    };
    let expressions = [
        expression(20, "p2", "5"),
        expression(21, "p2", "7"),
        expression(90, "p9", "p2 * 2"),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::native::attach_expression_parameters(&mut ir, &expressions, &[], &[], &mut annotations);

    assert!(ir.model.parameters[2].dependencies.is_empty());
}

#[test]
fn nx_formula_dependencies_resolve_within_the_expression_table() {
    let expression = |id: &str, table: &str, name: &str, text: &str, source_offset: u64| {
        crate::native::Expression {
            id: format!("nx:test:expression#{id}"),
            object_id: None,
            record: None,
            declaration: None,
            name: name.into(),
            parameter_index: None,
            qualifier: None,
            unit: crate::native::ExpressionUnit::Millimeter,
            expression: text.into(),
            value: None,
            source_entry: "/Root/UG_PART/UG_PART".into(),
            source_table: table.into(),
            source_offset,
        }
    };
    let expressions = [
        expression("a-p3", "table-a", "p3", "p2 * 2", 40),
        expression("b-p3", "table-b", "p3", "p2 * 2", 10),
        expression("a-p2", "table-a", "p2", "5", 30),
        expression("b-p2", "table-b", "p2", "7", 20),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

    crate::native::attach_expression_parameters(&mut ir, &expressions, &[], &[], &mut annotations);

    assert_eq!(ir.model.features.len(), 2);
    assert_eq!(ir.model.features[0].id.0, "table-b:feature#equations");
    assert_eq!(ir.model.features[0].ordinal, 0);
    assert_eq!(ir.model.features[1].id.0, "table-a:feature#equations");
    assert_eq!(ir.model.features[1].ordinal, 1);
    assert_eq!(
        ir.model
            .parameters
            .iter()
            .map(|parameter| (parameter.name.as_str(), parameter.ordinal))
            .collect::<Vec<_>>(),
        [("p2", 0), ("p3", 1), ("p2", 0), ("p3", 1)]
    );
    assert_eq!(ir.model.parameters[1].owner, ir.model.parameters[0].owner);
    assert_eq!(
        ir.model.parameters[1].dependencies,
        [ir.model.parameters[0].id.clone()]
    );
    assert_eq!(ir.model.parameters[3].owner, ir.model.parameters[2].owner);
    assert_eq!(
        ir.model.parameters[3].dependencies,
        [ir.model.parameters[2].id.clone()]
    );
    assert_ne!(ir.model.parameters[1].owner, ir.model.parameters[3].owner);
    for parameter in &mut ir.model.parameters {
        parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(1.0),
        ));
    }
    assert!(crate::decode::incomplete_expression_parameters(&ir).is_empty());

    let mut duplicate_name = ir.clone();
    duplicate_name.model.parameters[1].name = duplicate_name.model.parameters[0].name.clone();
    assert_eq!(
        crate::decode::incomplete_expression_parameters(&duplicate_name),
        duplicate_name.model.parameters[..2]
            .iter()
            .map(|parameter| parameter.id.clone())
            .collect()
    );

    let mut unevaluated = ir.clone();
    unevaluated.model.parameters[1].value = None;
    assert_eq!(
        crate::decode::incomplete_expression_parameters(&unevaluated),
        [unevaluated.model.parameters[1].id.clone()].into()
    );

    let mut operation_owned = unevaluated;
    operation_owned.model.features[0].definition =
        cadmpeg_ir::features::FeatureDefinition::Native {
            kind: "TEST_OPERATION".into(),
            properties: Default::default(),
            parameters: Default::default(),
        };
    assert_eq!(
        crate::decode::incomplete_expression_parameters(&operation_owned),
        [operation_owned.model.parameters[1].id.clone()].into()
    );
}

#[test]
fn nx_cyclic_formula_table_omits_invalid_neutral_dependency_edges() {
    let expression = |id: &str, name: &str, text: &str, source_offset| crate::native::Expression {
        id: format!("nx:test:expression#{id}"),
        object_id: None,
        record: None,
        declaration: None,
        name: name.to_string(),
        parameter_index: None,
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: text.to_string(),
        value: None,
        source_entry: "part".to_string(),
        source_table: "table".to_string(),
        source_offset,
    };
    let expressions = [
        expression("p2", "p2", "p3 + 1", 10),
        expression("p3", "p3", "p2 + 1", 20),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::native::attach_expression_parameters(&mut ir, &expressions, &[], &[], &mut annotations);

    assert_eq!(ir.model.parameters[0].expression, "p3 + 1");
    assert_eq!(ir.model.parameters[1].expression, "p2 + 1");
    assert!(ir
        .model
        .parameters
        .iter()
        .all(|parameter| parameter.dependencies.is_empty()));
    assert_eq!(
        crate::decode::incomplete_expression_parameters(&ir),
        ir.model
            .parameters
            .iter()
            .map(|parameter| parameter.id.clone())
            .collect()
    );
    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("2 NX expression parameter(s)"));
}

#[test]
fn nx_cyclic_formula_table_retains_independent_acyclic_dependencies() {
    let expression = |id: &str, name: &str, text: &str, source_offset| crate::native::Expression {
        id: format!("nx:test:expression#{id}"),
        object_id: None,
        record: None,
        declaration: None,
        name: name.to_string(),
        parameter_index: None,
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: text.to_string(),
        value: None,
        source_entry: "part".to_string(),
        source_table: "table".to_string(),
        source_offset,
    };
    let expressions = [
        expression("p2", "p2", "p3 + 1", 10),
        expression("p3", "p3", "p2 + 1", 20),
        expression("p5", "p5", "p4 * 2", 40),
        expression("p4", "p4", "7", 30),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

    crate::native::attach_expression_parameters(&mut ir, &expressions, &[], &[], &mut annotations);

    assert_eq!(
        ir.model
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        ["p4", "p5", "p2", "p3"]
    );
    assert_eq!(
        ir.model.parameters[1].dependencies,
        [ir.model.parameters[0].id.clone()]
    );
    assert!(ir.model.parameters[2].dependencies.is_empty());
    assert!(ir.model.parameters[3].dependencies.is_empty());
    for parameter in &mut ir.model.parameters {
        parameter.value = Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::features::Length(1.0),
        ));
    }
    assert_eq!(
        crate::decode::incomplete_expression_parameters(&ir),
        ir.model.parameters[2..]
            .iter()
            .map(|parameter| parameter.id.clone())
            .collect()
    );
}

#[test]
fn nx_parameter_uses_group_binding_witnesses_and_project_consumers() {
    use crate::native::{feature_parameter_uses, FeatureParameterBinding};

    let binding = |id: &str, operation: &str, slot: u8, offset: u64| FeatureParameterBinding {
        id: id.to_string(),
        operation_label: operation.to_string(),
        input_slot: slot,
        input_block: format!("block-{slot}"),
        reference_ordinal: 0,
        expression_declaration: "declaration".to_string(),
        expression: Some("nx:test:expression#20".to_string()),
        object_id: 20,
        source_offset: offset,
    };
    let uses = feature_parameter_uses(&[
        binding("late", "nx:feature-history:operation-label#1-2", 1, 30),
        binding("early", "nx:feature-history:operation-label#1-2", 0, 20),
        binding("other", "nx:feature-history:operation-label#1-3", 0, 40),
    ]);
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].bindings, ["early", "late"]);
    assert_eq!(uses[0].source_offsets, [20, 30]);

    let expression = crate::native::Expression {
        id: "nx:test:expression#20".to_string(),
        object_id: Some(20),
        record: None,
        declaration: None,
        name: "p20".to_string(),
        parameter_index: Some(20),
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: "5".to_string(),
        value: Some(5.0),
        source_entry: "part".to_string(),
        source_table: "table".to_string(),
        source_offset: 20,
    };
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::native::attach_expression_parameters(
        &mut ir,
        &[expression],
        &[],
        &uses,
        &mut annotations,
    );
    assert_eq!(
        ir.model.parameters[0].properties["consumer.0"],
        "nx:feature-history:feature#1-2"
    );
    assert_eq!(
        ir.model.parameters[0].properties["consumer.1"],
        "nx:feature-history:feature#1-3"
    );
}

#[test]
fn nx_parameter_consumers_follow_physical_use_order() {
    let expression = crate::native::Expression {
        id: "nx:test:expression#20".to_string(),
        object_id: Some(20),
        record: None,
        declaration: None,
        name: "p20".to_string(),
        parameter_index: Some(20),
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: "5".to_string(),
        value: Some(5.0),
        source_entry: "part".to_string(),
        source_table: "table".to_string(),
        source_offset: 10,
    };
    let parameter_use =
        |id: &str, operation: &str, source_offset| crate::native::FeatureParameterUse {
            id: id.to_string(),
            operation_label: operation.to_string(),
            expression: expression.id.clone(),
            bindings: vec![format!("binding-{id}")],
            source_offsets: vec![source_offset],
        };
    let uses = [
        parameter_use("later", "nx:feature-history:operation-label#0-1", 40),
        parameter_use("earlier", "nx:feature-history:operation-label#9-8", 30),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::native::attach_expression_parameters(
        &mut ir,
        &[expression],
        &[],
        &uses,
        &mut annotations,
    );

    assert_eq!(
        ir.model.parameters[0].properties["parameter_use.0"],
        "earlier"
    );
    assert_eq!(
        ir.model.parameters[0].properties["parameter_use.1"],
        "later"
    );
}

#[test]
fn nx_parameter_consumers_depend_on_preceding_expression_owner() {
    let expression = crate::native::Expression {
        id: "nx:test:expression#20".to_string(),
        object_id: Some(20),
        record: None,
        declaration: None,
        name: "p20".to_string(),
        parameter_index: Some(20),
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: "5".to_string(),
        value: Some(5.0),
        source_entry: "part".to_string(),
        source_table: "table".to_string(),
        source_offset: 20,
    };
    let parameter_use = crate::native::FeatureParameterUse {
        id: "use".to_string(),
        operation_label: "nx:feature-history:operation-label#1-2".to_string(),
        expression: expression.id.clone(),
        bindings: vec!["binding".to_string()],
        source_offsets: vec![30],
    };
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::native::attach_expression_parameters(
        &mut ir,
        &[expression],
        &[],
        std::slice::from_ref(&parameter_use),
        &mut annotations,
    );
    let parameter_owners = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect();
    let dependencies = crate::native::parameter_owner_dependencies(
        &parameter_owners,
        &[
            cadmpeg_ir::features::FeatureSourceContent::Parameter(
                cadmpeg_ir::features::ParameterId("nx:test:parameter#20".into()),
            ),
            cadmpeg_ir::features::FeatureSourceContent::Parameter(
                cadmpeg_ir::features::ParameterId("nx:test:parameter#20".into()),
            ),
        ],
    );

    assert_eq!(ir.model.features[0].ordinal, 0);
    assert_eq!(dependencies, [ir.model.parameters[0].owner.clone()]);
}

#[test]
fn nx_feature_source_content_orders_parameter_occurrences_with_text() {
    let text = crate::native::FeaturePayloadString {
        id: "text".into(),
        operation_record: "record".into(),
        ordinal: 0,
        value: "Through".into(),
        source_offset: 30,
    };
    let parameter_use = crate::native::FeatureParameterUse {
        id: "use".into(),
        operation_label: "operation".into(),
        expression: "nx:test:expression#20".into(),
        bindings: vec!["first".into(), "second".into()],
        source_offsets: vec![20, 40],
    };
    let content = crate::native::feature_source_content(&[&text], &[&parameter_use]);
    assert_eq!(content.len(), 3);
    assert!(matches!(
        &content[0],
        cadmpeg_ir::features::FeatureSourceContent::Parameter(id)
            if id.0 == "nx:test:parameter#20"
    ));
    assert!(matches!(
        &content[1],
        cadmpeg_ir::features::FeatureSourceContent::Text(value) if value == "Through"
    ));
    assert!(matches!(
        &content[2],
        cadmpeg_ir::features::FeatureSourceContent::Parameter(id)
            if id.0 == "nx:test:parameter#20"
    ));
}

#[test]
fn nx_native_feature_parameters_require_unique_resolved_names() {
    let expression = |id: &str, name: &str, text: &str| crate::native::Expression {
        id: id.to_string(),
        object_id: None,
        record: None,
        declaration: None,
        name: name.to_string(),
        parameter_index: None,
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: text.to_string(),
        value: None,
        source_entry: "entry".to_string(),
        source_table: "table".to_string(),
        source_offset: 0,
    };
    let parameter_use = |id: &str, expression: &str| crate::native::FeatureParameterUse {
        id: id.to_string(),
        operation_label: "operation".to_string(),
        expression: expression.to_string(),
        bindings: vec![format!("binding-{id}")],
        source_offsets: vec![0],
    };
    let expressions = vec![
        expression("expression-a", "p1_length", "p2_length * 2"),
        expression("expression-b", "p2_length", "12.5"),
    ];
    let uses = [
        parameter_use("use-a", "expression-a"),
        parameter_use("use-b", "expression-b"),
    ];
    let use_refs = uses.iter().collect::<Vec<_>>();
    let parameters = crate::native::native_feature_parameters(&use_refs, &expressions);
    assert_eq!(
        parameters,
        std::collections::BTreeMap::from([
            ("p1_length".to_string(), "p2_length * 2".to_string()),
            ("p2_length".to_string(), "12.5".to_string()),
        ])
    );
    assert_eq!(
        crate::native::non_boolean_feature_definition_with_parameters(
            "UNKNOWN OPERATION",
            &[],
            None,
            None,
            crate::native::HoleProjection::default(),
            parameters,
        ),
        cadmpeg_ir::features::FeatureDefinition::Native {
            kind: "UNKNOWN OPERATION".to_string(),
            parameters: std::collections::BTreeMap::from([
                ("p1_length".to_string(), "p2_length * 2".to_string()),
                ("p2_length".to_string(), "12.5".to_string()),
            ]),
            properties: std::collections::BTreeMap::new(),
        }
    );
    assert!(matches!(
        crate::native::non_boolean_feature_definition_with_parameters(
            "DELETE",
            &[],
            None,
            None,
            crate::native::HoleProjection::default(),
            Default::default(),
        ),
        cadmpeg_ir::features::FeatureDefinition::Native { kind, .. } if kind == "DELETE"
    ));

    let duplicate_expressions = vec![
        expression("expression-a", "p1_length", "1"),
        expression("expression-b", "p1_length", "2"),
    ];
    assert!(crate::native::native_feature_parameters(&use_refs, &duplicate_expressions).is_empty());
    let unresolved = [parameter_use("use-c", "missing")];
    assert!(crate::native::native_feature_parameters(
        &unresolved.iter().collect::<Vec<_>>(),
        &expressions,
    )
    .is_empty());
}

#[test]
fn nx_hole_completeness_accepts_independent_placement_and_rejects_opaque_operands() {
    use cadmpeg_ir::features::{Extent, FaceSelection, HoleKind, Length, ProfileRef};
    use cadmpeg_ir::math::{Point3, Vector3};

    assert!(!crate::decode::hole_feature_is_incomplete(
        None,
        None,
        Some(Point3::new(1.0, 2.0, 3.0)),
        Some(Vector3::new(0.0, 0.0, 1.0)),
        (&HoleKind::Simple, None),
        Some(Length(5.0)),
        Some(&Extent::ThroughAll),
    ));
    assert!(crate::decode::hole_feature_is_incomplete(
        Some(&ProfileRef::Unresolved),
        Some(&FaceSelection::Unresolved),
        None,
        None,
        (&HoleKind::Simple, None),
        Some(Length(5.0)),
        Some(&Extent::ThroughAll),
    ));
    assert!(crate::decode::hole_feature_is_incomplete(
        None,
        None,
        Some(Point3::new(1.0, 2.0, 3.0)),
        Some(Vector3::new(0.0, 0.0, 1.0)),
        (&HoleKind::Simple, None),
        Some(Length(5.0)),
        Some(&Extent::Unresolved),
    ));
    assert!(crate::decode::hole_feature_is_incomplete(
        None,
        None,
        Some(Point3::new(1.0, 2.0, 3.0)),
        Some(Vector3::new(0.0, 0.0, 1.0)),
        (
            &HoleKind::Simple,
            Some(&HoleKind::Unresolved {
                form: Some(cadmpeg_ir::features::HoleForm::Chamfer),
                counterbore_diameter: None,
                counterbore_depth: None,
                countersink_diameter: None,
                countersink_angle: None,
            }),
        ),
        Some(Length(5.0)),
        Some(&Extent::ThroughAll),
    ));
}

#[test]
fn nx_extent_completeness_checks_nested_and_face_termination() {
    use cadmpeg_ir::features::{Extent, FaceSelection, Length};

    assert!(!crate::decode::extent_is_incomplete(
        &Extent::TwoSidedExtents {
            first: Box::new(Extent::Blind {
                length: Length(5.0),
            }),
            second: Box::new(Extent::ThroughAll),
        }
    ));
    assert!(crate::decode::extent_is_incomplete(
        &Extent::SymmetricExtent {
            extent: Box::new(Extent::Unresolved),
        }
    ));
    assert!(crate::decode::extent_is_incomplete(&Extent::ToFace {
        face: FaceSelection::Native("nx:face-selection#0".to_string()),
    }));
    assert!(crate::decode::extent_is_incomplete(&Extent::ToShape {
        target: FaceSelection::Resolved {
            faces: Vec::new(),
            native: "nx:face-selection#1".to_string(),
        },
    }));
}

#[test]
fn nx_rib_completeness_requires_a_resolved_profile() {
    use cadmpeg_ir::features::{BooleanOp, Length, ProfileRef, RibConstruction, RibDraft, RibSide};
    use cadmpeg_ir::math::Vector3;

    let mut construction = RibConstruction {
        profile: Some(ProfileRef::Native("nx:profile#0".to_string())),
        direction: Some(Vector3::new(0.0, 0.0, 1.0)),
        thickness: Some(Length(2.0)),
        side: Some(RibSide::Centered),
        draft: RibDraft::None,
    };
    assert!(crate::decode::rib_feature_is_incomplete(
        &construction,
        BooleanOp::Join,
    ));
    construction.profile = Some(ProfileRef::Faces(vec![cadmpeg_ir::ids::FaceId(
        "face#0".to_string(),
    )]));
    assert!(!crate::decode::rib_feature_is_incomplete(
        &construction,
        BooleanOp::Join,
    ));
    construction.profile = Some(ProfileRef::Faces(Vec::new()));
    assert!(crate::decode::rib_feature_is_incomplete(
        &construction,
        BooleanOp::Join,
    ));
}

#[test]
fn nx_chamfer_direction_is_required_only_for_asymmetric_specs() {
    use cadmpeg_ir::features::{Angle, ChamferSpec, Length};

    assert!(!crate::decode::chamfer_requires_direction(
        &ChamferSpec::Distance {
            distance: Length(2.0),
        }
    ));
    assert!(!crate::decode::chamfer_requires_direction(
        &ChamferSpec::TwoDistances {
            first: Length(2.0),
            second: Length(2.0),
        }
    ));
    assert!(crate::decode::chamfer_requires_direction(
        &ChamferSpec::TwoDistances {
            first: Length(2.0),
            second: Length(3.0),
        }
    ));
    assert!(crate::decode::chamfer_requires_direction(
        &ChamferSpec::DistanceAngle {
            distance: Length(2.0),
            angle: Angle(0.5),
        }
    ));
}

#[test]
fn nx_pattern_completeness_requires_every_regeneration_operand() {
    use cadmpeg_ir::features::{
        Length, PathRef, PatternKind, PatternStage, PatternStageCombination,
    };
    use cadmpeg_ir::math::Vector3;

    let linear = PatternKind::Linear {
        direction: Some(Vector3::new(1.0, 0.0, 0.0)),
        spacing: Length(10.0),
        count: 3,
    };
    assert!(!crate::decode::pattern_is_incomplete(&linear));
    assert!(crate::decode::pattern_is_incomplete(&PatternKind::Linear {
        direction: None,
        spacing: Length(10.0),
        count: 3,
    }));
    assert!(crate::decode::pattern_is_incomplete(
        &PatternKind::CurveDriven {
            path: Some(PathRef::Native("nx:path".into())),
            spacing: Length(10.0),
            count: 3,
        }
    ));
    assert!(crate::decode::pattern_is_incomplete(
        &PatternKind::Composite {
            stages: vec![PatternStage {
                pattern: Box::new(PatternKind::Linear {
                    direction: None,
                    spacing: Length(10.0),
                    count: 3,
                }),
                combination: PatternStageCombination::Initialize,
            }],
        }
    ));
}

#[test]
fn nx_variable_radius_completeness_requires_a_law_interval() {
    use cadmpeg_ir::features::{Length, RadiusSpec, VariableRadius};

    assert!(crate::decode::radius_spec_is_incomplete(
        &RadiusSpec::Variable { points: Vec::new() }
    ));
    assert!(crate::decode::radius_spec_is_incomplete(
        &RadiusSpec::Variable {
            points: vec![VariableRadius {
                parameter: 0.0,
                radius: Length(2.0),
            }],
        }
    ));
    assert!(!crate::decode::radius_spec_is_incomplete(
        &RadiusSpec::Variable {
            points: vec![
                VariableRadius {
                    parameter: 0.0,
                    radius: Length(2.0),
                },
                VariableRadius {
                    parameter: 1.0,
                    radius: Length(3.0),
                },
            ],
        }
    ));
    assert!(!crate::decode::radius_spec_is_incomplete(
        &RadiusSpec::Constant {
            radius: Length(2.0),
        }
    ));
}

#[test]
fn nx_empty_resolved_selections_remain_incomplete() {
    use cadmpeg_ir::features::{BodySelection, EdgeSelection, FaceSelection, PathRef, ProfileRef};

    assert!(crate::decode::body_selection_is_incomplete(
        &BodySelection::Bodies(Vec::new())
    ));
    assert!(crate::decode::face_selection_is_incomplete(
        &FaceSelection::Resolved {
            faces: Vec::new(),
            native: "nx:faces".into(),
        }
    ));
    assert!(crate::decode::edge_selection_is_incomplete(
        &EdgeSelection::Edges(Vec::new())
    ));
    assert!(!crate::decode::edge_selection_is_incomplete(
        &EdgeSelection::All
    ));
    assert!(crate::decode::profile_ref_is_incomplete(
        &ProfileRef::Faces(Vec::new())
    ));
    assert!(crate::decode::path_ref_is_incomplete(&PathRef::Curves(
        Vec::new()
    )));
    let edge = cadmpeg_ir::ids::EdgeId("edge#0".into());
    assert!(crate::decode::path_ref_is_incomplete(&PathRef::Edges(
        vec![edge.clone(), edge]
    )));
    let curve = cadmpeg_ir::ids::CurveId("curve#0".into());
    assert!(crate::decode::path_ref_is_incomplete(&PathRef::Curves(
        vec![curve.clone(), curve]
    )));
}

#[test]
fn complete_extrude_profile_projects_without_guessing_scalar_roles() {
    use cadmpeg_ir::features::{BooleanOp, Extent, FeatureDefinition, ProfileRef};

    assert_eq!(
        crate::native::extrude_feature_definition(Some("nx:profile#1"), None, BooleanOp::NewBody,),
        Some(FeatureDefinition::Extrude {
            profile: ProfileRef::Native("nx:profile#1".to_string()),
            direction: None,
            extent: Extent::Unresolved,
            op: BooleanOp::NewBody,
            draft: None,
            reverse_draft: None,
            direction_source: None,
            solid: None,
            face_maker: None,
            inner_wire_taper: None,
            first_offset: None,
            second_offset: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        })
    );
    assert!(crate::native::extrude_feature_definition(None, None, BooleanOp::Unresolved).is_none());
    assert!(crate::native::extrude_feature_definition(
        Some("nx:profile#1"),
        Some("nx:profile#2"),
        BooleanOp::Unresolved,
    )
    .is_none());
}

#[test]
fn extrusion_is_new_body_only_for_one_first_written_surface_or_solid_output() {
    use cadmpeg_ir::features::BooleanOp;
    use cadmpeg_ir::topology::BodyKind;

    assert_eq!(
        crate::native::extrude_boolean_op(false, &[BodyKind::Solid]),
        BooleanOp::NewBody
    );
    assert_eq!(
        crate::native::extrude_boolean_op(true, &[BodyKind::Solid]),
        BooleanOp::Unresolved
    );
    assert_eq!(
        crate::native::extrude_boolean_op(false, &[BodyKind::Sheet]),
        BooleanOp::NewBody
    );
    assert_eq!(
        crate::native::extrude_boolean_op(false, &[BodyKind::Wire]),
        BooleanOp::Unresolved
    );
    assert_eq!(
        crate::native::extrude_boolean_op(false, &[BodyKind::General]),
        BooleanOp::Unresolved
    );
    assert_eq!(
        crate::native::extrude_boolean_op(false, &[BodyKind::Solid, BodyKind::Solid]),
        BooleanOp::Unresolved
    );
    assert_eq!(
        crate::native::extrude_boolean_op(false, &[]),
        BooleanOp::Unresolved
    );
}

#[test]
fn nx_block_source_content_includes_complete_ordered_dimension_run() {
    use cadmpeg_ir::features::{FeatureSourceContent, ParameterId};

    let mut content = vec![FeatureSourceContent::Parameter(ParameterId(
        "nx:test:parameter#20".into(),
    ))];
    crate::native::append_feature_expression_content(
        &mut content,
        &[
            "nx:test:expression#20".into(),
            "nx:test:expression#21".into(),
            "nx:test:expression#22".into(),
        ],
    );
    assert_eq!(
        content,
        [
            FeatureSourceContent::Parameter(ParameterId("nx:test:parameter#20".into())),
            FeatureSourceContent::Parameter(ParameterId("nx:test:parameter#21".into())),
            FeatureSourceContent::Parameter(ParameterId("nx:test:parameter#22".into())),
        ]
    );
}

#[test]
fn nx_block_dimensions_do_not_cross_expression_sections() {
    use crate::native::{
        Expression, ExpressionDeclaration, ExpressionUnit, FeatureBlockConstruction,
        FeatureParameterBinding,
    };

    let operation = "nx:feature-history:operation-label#0-1";
    let construction = FeatureBlockConstruction {
        id: "nx:feature-history:block-construction#0-1".into(),
        operation_label: operation.into(),
        control: 0,
        member_references: Vec::new(),
        member_data_blocks: Vec::new(),
        terminal_reference: "terminal-reference".into(),
        terminal_data_block: "terminal-block".into(),
    };
    let binding = FeatureParameterBinding {
        id: "binding".into(),
        operation_label: operation.into(),
        input_slot: 0,
        input_block: "input".into(),
        reference_ordinal: 0,
        expression_declaration: "declaration-20".into(),
        expression: Some("expression-20".into()),
        object_id: 20,
        source_offset: 1,
    };
    let declaration = |index: u32, source_entry: &str| ExpressionDeclaration {
        id: format!("declaration-{index}"),
        object_id: index,
        record: format!("{source_entry}:entry#{index}"),
        name: format!("p{index}"),
        parameter_index: index,
        qualifier: None,
        literal: None,
        source_entry: source_entry.into(),
        source_offset: u64::from(index),
    };
    let expression = |index: u32, source_entry: &str, source_table: &str| Expression {
        id: format!("expression-{index}"),
        object_id: Some(index),
        record: Some(format!("{source_entry}:entry#{index}")),
        declaration: Some(format!("declaration-{index}")),
        name: format!("p{index}"),
        parameter_index: Some(index),
        qualifier: None,
        unit: ExpressionUnit::Millimeter,
        expression: index.to_string(),
        value: Some(f64::from(index)),
        source_entry: source_entry.into(),
        source_table: source_table.into(),
        source_offset: u64::from(index),
    };
    let mut expressions = [
        expression(20, "section-a", "table-a"),
        expression(21, "section-a", "table-a"),
        expression(22, "section-b", "table-b"),
    ];
    let mut declarations = [
        declaration(20, "section-a"),
        declaration(21, "section-a"),
        declaration(22, "section-b"),
    ];

    assert!(crate::native::feature_block_dimensions(
        std::slice::from_ref(&construction),
        std::slice::from_ref(&binding),
        &declarations,
        &expressions,
    )
    .is_empty());

    declarations[2].source_entry = "section-a".into();
    declarations[2].record = "section-a:entry#22".into();
    assert!(crate::native::feature_block_dimensions(
        std::slice::from_ref(&construction),
        std::slice::from_ref(&binding),
        &declarations,
        &expressions,
    )
    .is_empty());

    expressions[2].source_entry = "section-a".into();
    expressions[2].source_table = "table-a".into();
    assert_eq!(
        crate::native::feature_block_dimensions(
            &[construction],
            &[binding],
            &declarations,
            &expressions,
        )
        .len(),
        1
    );
}

#[test]
fn nx_block_dimension_parameters_name_the_block_as_consumer() {
    let expression = |key: u32| crate::native::Expression {
        id: format!("nx:test:expression#{key}"),
        object_id: Some(key),
        record: None,
        declaration: None,
        name: format!("p{key}"),
        parameter_index: Some(key),
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: key.to_string(),
        value: Some(f64::from(key)),
        source_entry: "part".into(),
        source_table: "table".into(),
        source_offset: u64::from(key),
    };
    let expressions = [expression(20), expression(21), expression(22)];
    let dimensions = crate::native::FeatureBlockDimensions {
        id: "dimensions".into(),
        operation_label: "nx:feature-history:operation-label#1-4".into(),
        construction: "construction".into(),
        anchor_bindings: vec!["binding".into()],
        declarations: ["d20".into(), "d21".into(), "d22".into()],
        expressions: [
            expressions[0].id.clone(),
            expressions[1].id.clone(),
            expressions[2].id.clone(),
        ],
        values: [20.0, 21.0, 22.0],
    };
    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::native::attach_expression_parameters(&mut ir, &expressions, &[], &[], &mut annotations);
    let parameter_owners = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect();
    let mut content = Vec::new();
    crate::native::append_feature_expression_content(&mut content, &dimensions.expressions);
    assert_eq!(
        crate::native::parameter_owner_dependencies(&parameter_owners, &content),
        [ir.model.features[0].id.clone()]
    );
    crate::native::attach_block_dimension_parameter_consumers(
        &mut ir,
        &[dimensions],
        &mut annotations,
    );
    assert_eq!(ir.model.parameters.len(), 3);
    for (ordinal, parameter) in ir.model.parameters.iter().enumerate() {
        assert_eq!(
            parameter.properties[&format!("block_dimension.{ordinal}")],
            "dimensions"
        );
        assert_eq!(
            parameter.properties["consumer.0"],
            "nx:feature-history:feature#1-4"
        );
    }
}

/// Write three big-endian doubles into `rec` starting at `at`.
fn put_vec3(rec: &mut [u8], at: usize, xyz: [f64; 3]) {
    for (i, v) in xyz.iter().enumerate() {
        rec[at + 8 * i..at + 8 * i + 8].copy_from_slice(&be_f64(*v));
    }
}

fn put_f64(rec: &mut [u8], at: usize, v: f64) {
    rec[at..at + 8].copy_from_slice(&be_f64(v));
}

fn put_ref(rec: &mut [u8], at: usize, value: u16) {
    rec[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

fn encoded_xmt(value: u32) -> Vec<u8> {
    if i16::try_from(value).is_ok() {
        return (value as u16).to_be_bytes().to_vec();
    }
    let quotient = value / 32_767;
    let remainder = value % 32_767;
    assert!(remainder > 0 && i16::try_from(remainder).is_ok());
    let mut out = (-(remainder as i16)).to_be_bytes().to_vec();
    out.extend_from_slice(&(quotient as u16).to_be_bytes());
    out
}

/// One fixed-length analytic record: a `00 <tag>` header then zeroed payload the
/// caller fills at the documented offsets.
fn record(tag: u8, len: usize) -> Vec<u8> {
    let mut r = vec![0u8; len];
    r[0] = 0x00;
    r[1] = tag;
    r
}

fn indexed_om_section() -> Vec<u8> {
    let mut bytes = vec![0xaa; 32];
    let base = 8usize;
    let class_name = b"UGS::EXP_expression";
    bytes[base] = (class_name.len() + 1) as u8;
    bytes[base + 1..base + 1 + class_name.len()].copy_from_slice(class_name);
    bytes[base + 1 + class_name.len()] = 0x81;
    let field_name = b"m_target";
    bytes.push((field_name.len() + 1) as u8);
    bytes.extend_from_slice(field_name);
    bytes.push(0x80);
    let root = b"\x04\x01\x0eNX 2027.3102\x00hostglobalvariables";
    let text = b"(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; ";
    let declaration_name = b"p8_CircularPattern_pattern_Circular_Dir_offset_angle";
    let mut expression = vec![0x04, (declaration_name.len() + 2) as u8];
    expression.extend_from_slice(declaration_name);
    expression.push(0);
    expression.extend_from_slice(b"\x04\x05120\0");
    expression.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    expression.extend_from_slice(text);
    expression.push(0);
    expression.extend_from_slice(b"\x66\x32\x03\x0cSKETCH_001\0");
    expression.extend_from_slice(b"\xe0\x12\x34\x56\x78\xca\xbc\xde\xf0");
    expression.extend_from_slice(b"\x01\x02\x90\x00\x00");
    let records = [root.as_slice(), expression.as_slice()];
    let table = bytes.len() + 4 * 4;
    let table_end = table + 4 + 3 * 4;
    let first = table_end - base;
    let second = first + records[0].len();
    let end = second + records[1].len();
    for value in [0u32, first as u32, second as u32, end as u32] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    for id in [0x100u32, 0x101, 0x102] {
        bytes.extend_from_slice(&id.to_le_bytes());
    }
    bytes.extend_from_slice(records[0]);
    bytes.extend_from_slice(records[1]);
    bytes
}

fn offset_only_indexed_om_section() -> Vec<u8> {
    let mut bytes = vec![0xaa; 8];
    let class_name = b"UGS::ModlFeature";
    bytes.push((class_name.len() + 1) as u8);
    bytes.extend_from_slice(class_name);
    bytes.push(0x81);
    let index_start = bytes.len();
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let metadata = bytes.len();
    bytes.extend_from_slice(&[0, 0, 0, 0, 0, 1, 0, 0]);
    let first = bytes.len();
    bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0hostglobalvariables");
    let second = bytes.len();
    let text = b"(Number [mm]) length: 25; ";
    bytes.extend_from_slice(&[0x04, 0x00, 0x2a, 0x02, 0x0b]);
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);
    let end = bytes.len();
    for (index, offset) in [metadata, first, second, end].into_iter().enumerate() {
        bytes[index_start + index * 4..index_start + index * 4 + 4]
            .copy_from_slice(&(offset as u32).to_le_bytes());
    }
    bytes
}

/// An offset-store indexed OM section whose control block is replaced by
/// `control_block`. The first record remains the single supported product
/// record (so the section validates), leaving the control block free to carry
/// persistent-handle references or an index-value array for the
/// `data_block_control_*` extractors.
fn offset_only_indexed_om_section_with_control(control_block: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xaa; 8];
    let class_name = b"UGS::ModlFeature";
    bytes.push((class_name.len() + 1) as u8);
    bytes.extend_from_slice(class_name);
    bytes.push(0x81);
    let index_start = bytes.len();
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let metadata = bytes.len();
    bytes.extend_from_slice(control_block);
    let first = bytes.len();
    bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0hostglobalvariables");
    let second = bytes.len();
    let text = b"(Number [mm]) length: 25; ";
    bytes.extend_from_slice(&[0x04, 0x00, 0x2a, 0x02, 0x0b]);
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);
    let end = bytes.len();
    for (index, offset) in [metadata, first, second, end].into_iter().enumerate() {
        bytes[index_start + index * 4..index_start + index * 4 + 4]
            .copy_from_slice(&(offset as u32).to_le_bytes());
    }
    bytes
}

/// An offset-store indexed OM section whose single product record lives inside
/// the control block, preceded by a zero-prefixed aligned index-value array.
/// The two column records carry no product marker, so the section still holds
/// exactly one product record and `data_block_control_index_values` decodes the
/// array. Mirrors the `om_offset_store_index_values_*` white-box test.
fn offset_only_indexed_om_section_with_index_values() -> Vec<u8> {
    let mut control = Vec::new();
    control.extend_from_slice(&[0, 0]); // two-byte zero prefix
    control.extend_from_slice(&7u32.to_le_bytes());
    control.extend_from_slice(&0x1020u32.to_le_bytes());
    control.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0"); // the one product record

    let mut bytes = vec![0xaa; 8];
    let class_name = b"UGS::ModlFeature";
    bytes.push((class_name.len() + 1) as u8);
    bytes.extend_from_slice(class_name);
    bytes.push(0x81);
    let index_start = bytes.len();
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let metadata = bytes.len();
    bytes.extend_from_slice(&control);
    let first = bytes.len();
    bytes.extend_from_slice(&[0xbb; 12]); // column record, no product marker
    let second = bytes.len();
    bytes.extend_from_slice(&[0xcc; 12]); // column record, no product marker
    let end = bytes.len();
    for (index, offset) in [metadata, first, second, end].into_iter().enumerate() {
        bytes[index_start + index * 4..index_start + index * 4 + 4]
            .copy_from_slice(&(offset as u32).to_le_bytes());
    }
    bytes
}

/// An offset-store indexed OM section whose first (object-id-less) record is an
/// offset-store named point (`Point7` with two `57.15` scalars). The single
/// product record lives in the control block so the section validates while the
/// column records carry the point payload. Mirrors the
/// `om_offset_store_named_point_*` white-box test.
fn offset_only_indexed_om_section_with_named_point() -> Vec<u8> {
    let mut named_point = vec![
        0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'7', 0x00, 0x50, 0x59, 0x66, 0x58, 0x00, 0x30,
        0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ];
    named_point.extend_from_slice(&[
        0x45, 0x04, 0x00, 0x50, 0x59, 0x66, 0x58, 0x00, 0x30, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33,
        0x07,
    ]);

    let mut bytes = vec![0xaa; 8];
    let class_name = b"UGS::ModlFeature";
    bytes.push((class_name.len() + 1) as u8);
    bytes.extend_from_slice(class_name);
    bytes.push(0x81);
    let index_start = bytes.len();
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let metadata = bytes.len();
    bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0"); // product record in control block
    let first = bytes.len();
    bytes.extend_from_slice(&named_point); // first record: the named point
    let second = bytes.len();
    bytes.extend_from_slice(&[0xbb; 8]); // trailing column record, no point payload
    let end = bytes.len();
    for (index, offset) in [metadata, first, second, end].into_iter().enumerate() {
        bytes[index_start + index * 4..index_start + index * 4 + 4]
            .copy_from_slice(&(offset as u32).to_le_bytes());
    }
    bytes
}

fn control_root_offset_only_indexed_om_section() -> Vec<u8> {
    let mut bytes = vec![0xaa; 8];
    let class_name = b"UGS::ModlFeature";
    bytes.push((class_name.len() + 1) as u8);
    bytes.extend_from_slice(class_name);
    bytes.push(0x81);
    let index_start = bytes.len();
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let control = bytes.len();
    bytes.extend_from_slice(&[0xf0, 1, 0, 0]);
    bytes.extend_from_slice(b"\x05\x01\x0eNX 2027.3102\0control-tail");
    let first = bytes.len();
    bytes.extend_from_slice(&[0; 32]);
    let second = bytes.len();
    let text = b"(Number [mm]) length: 25; ";
    bytes.extend_from_slice(b"hostglobalvariables");
    bytes.extend_from_slice(&[0x04, 0x00, 0x2a, 0x02, 0x0b]);
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);
    let end = bytes.len();
    for (index, offset) in [control, first, second, end].into_iter().enumerate() {
        bytes[index_start + index * 4..index_start + index * 4 + 4]
            .copy_from_slice(&(offset as u32).to_le_bytes());
    }
    bytes
}

fn size_framed_om_section() -> Vec<u8> {
    let mut bytes = vec![0xff; 16];
    bytes[4..8].fill(0);
    bytes[12..14].copy_from_slice(b"OM");
    bytes.extend_from_slice(&[0, 1, 2]);
    for (index, (name, code)) in [
        (b"UGS::FEATURE_RECORD".as_slice(), 0xa0),
        (b"UGS::ModlUtils::BooleanComponent".as_slice(), 0x65),
    ]
    .into_iter()
    .enumerate()
    {
        bytes.push((name.len() + 1) as u8);
        bytes.extend_from_slice(name);
        bytes.push(code);
        if index == 0 {
            bytes.extend_from_slice(&[
                0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06,
            ]);
        }
    }
    for (name, code, suffix) in [
        (b"m_target".as_slice(), 0x80, [0x01, 0x02]),
        (b"m_tools".as_slice(), 0x81, [0x03, 0x04]),
    ] {
        bytes.push((name.len() + 1) as u8);
        bytes.extend_from_slice(name);
        bytes.push(code);
        bytes.extend_from_slice(&suffix);
    }
    bytes.extend_from_slice(b"unframed UGS::PayloadText");
    let payload_len = (bytes.len() - 16) as u32;
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());
    bytes
}

fn size_framed_om_section_with_record_area() -> Vec<u8> {
    let mut bytes = size_framed_om_section();
    let record_area = bytes.len() + 20;
    bytes.extend_from_slice(&(record_area as u32).to_le_bytes());
    bytes.resize(record_area, 0);
    bytes.extend_from_slice(&13u32.to_le_bytes());
    bytes.extend_from_slice(&14u32.to_le_bytes());
    bytes.extend_from_slice(&44u32.to_le_bytes());
    bytes.extend_from_slice(b"\x05\x01\x0eNX 2027.3102\0feature-records\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x01\x82\x40\x90\x17\xd3\xff\x03\x07UNITE\0\x31\x00\x00\x01\x00\x14\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\x00\x00\xe0\x7f\xff\xff\xff\x01\x01\x01\x02\x90\x19\x42\x00\x01\x03\x90\x19\x4c\x7f\x00\x01\x02\x10\x90\x19\x42\xff");
    let payload_len = (bytes.len() - 16) as u32;
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());
    bytes
}

fn size_framed_om_section_with_repeated_operations(count: usize) -> Vec<u8> {
    let section = size_framed_om_section_with_record_area();
    let operation = section
        .windows(15)
        .position(|window| {
            window == b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff"
        })
        .expect("operation marker");
    let mut bytes = section[..operation].to_vec();
    for _ in 0..count {
        bytes.extend_from_slice(&section[operation..]);
    }
    let payload_len = (bytes.len() - 16) as u32;
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());
    bytes
}

#[test]
fn om_index_pairs_object_ids_with_bounded_entity_records() {
    let bytes = indexed_om_section();
    let sections = crate::om::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].base, 8);
    assert_eq!(sections[0].records.len(), 2);
    assert_eq!(sections[0].records[0].object_id, Some(0x101));
    assert_eq!(
        sections[0].records[0].object_id_offset,
        Some(sections[0].object_id_table_offset + 8)
    );
    assert_eq!(
        sections[0].records[0].bytes,
        b"\x04\x01\x0eNX 2027.3102\x00hostglobalvariables"
    );
    assert_eq!(sections[0].records[1].object_id, Some(0x102));
    assert_eq!(
        sections[0].records[1].object_id_offset,
        Some(sections[0].object_id_table_offset + 12)
    );
    assert_eq!(sections[0].column_storage, None);
    assert_eq!(sections[0].fields.len(), 1);
    assert_eq!(sections[0].fields[0].name, "m_target");
    assert_eq!(
        sections[0].records[1].bytes,
        b"\x04\x36p8_CircularPattern_pattern_Circular_Dir_offset_angle\x00\x04\x05120\x00\x99\x04P(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; \x00\x66\x32\x03\x0cSKETCH_001\0\xe0\x12\x34\x56\x78\xca\xbc\xde\xf0\x01\x02\x90\x00\x00"
    );
}

#[test]
fn ug_part_segment_index_uses_row_one_self_boundary() {
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
    let container = container::scan_bytes(file).unwrap();
    let (_, index) = container.segment_index().expect("segment index");
    assert_eq!(index.byte_len, 28);
    assert_eq!(index.rows.len(), 2);
    assert_eq!(index.rows[0].type_code, 7);
    assert_eq!(index.rows[0].subtype_code, 9);
    assert_eq!(index.rows[0].value, 11);
    assert_eq!(index.rows[1].type_code, 1);
    assert_eq!(index.rows[1].subtype_code, 1);
    assert_eq!(index.rows[1].value, 28);
    assert_eq!(index.padding, &[0xaa, 0xbb, 0xcc, 0xdd]);
}

#[test]
fn decode_retains_ordered_ug_part_segment_index_rows() {
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let namespace = result.ir.native.namespace("nx").expect("NX namespace");
    assert_eq!(namespace.version, 155);
    let rows = namespace
        .arena_as::<crate::native::SegmentIndexRow>("segment_index_rows")
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].ordinal, 0);
    assert_eq!(rows[1].value, 28);
    assert_eq!(rows[1].source_entry, "/Root/UG_PART/UG_PART");
    assert_eq!(rows[1].source_offset, rows[0].source_offset + 12);
}

#[test]
fn decode_links_segment_index_word_to_validated_stream_wrapper() {
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_stream_payload())]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let links = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::SegmentStreamLink>("segment_stream_links")
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].row, "nx:segment-index:row#0");
    assert_eq!(links[0].slot, crate::native::SegmentIndexSlot::TypeCode);
    assert_eq!(links[0].stream_ordinal, 0);
    assert_eq!(links[0].stream_kind, "deltas");
    assert_eq!(links[0].wrapper_byte_len, 8);
}

#[test]
fn decode_binds_segment_body_object_index_to_partition_stream() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        segment_body_binding_payload("partition"),
    )]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let bindings = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::SegmentBodyBinding>("segment_body_bindings")
        .unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].stream_ordinal, 0);
    assert_eq!(bindings[0].stream_kind, "partition");
    assert_eq!(bindings[0].body_object_index, 94);
    assert_eq!(bindings[0].body_alias_object_index, 150);
    assert_eq!(bindings[0].stream_role, 19);
    assert_eq!(bindings[0].source_offset, 104);
}

#[test]
fn decode_binds_segment_body_object_index_to_plain_cached_body_stream() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        segment_body_binding_payload("plain"),
    )]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let bindings = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::SegmentBodyBinding>("segment_body_bindings")
        .unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].stream_ordinal, 0);
    assert_eq!(bindings[0].stream_kind, "plain");
    assert_eq!(bindings[0].body_object_index, 94);
    assert_eq!(bindings[0].body_alias_object_index, 150);
    assert_eq!(bindings[0].stream_role, 19);
}

#[test]
fn decode_links_extended_partition_wrapper_and_body_identity() {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_extended_wrapper_payload())]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let namespace = result.ir.native.namespace("nx").unwrap();
    let links = namespace
        .arena_as::<crate::native::SegmentStreamLink>("segment_stream_links")
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].wrapper_byte_len, 38);
    let bindings = namespace
        .arena_as::<crate::native::SegmentBodyBinding>("segment_body_bindings")
        .unwrap();
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].body_object_index, 94);
    assert_eq!(bindings[0].body_alias_object_index, 150);
    assert_eq!(bindings[0].stream_role, 19);
}

#[test]
fn feature_body_selection_resolves_complete_segment_bindings_atomically() {
    use cadmpeg_ir::features::BodySelection;
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let first = BodyId("nx:s2:body#3".to_string());
    let second = BodyId("nx:s4:body#3".to_string());
    let bindings = BTreeMap::from([(94, vec![first.clone()]), (122, vec![second.clone()])]);
    assert_eq!(
        crate::native::feature_body_selection(
            &[94, 122],
            &bindings,
            "nx:om-object-indices#94,122".to_string(),
        ),
        BodySelection::Resolved {
            bodies: vec![first.clone(), second],
            native: "nx:om-object-indices#94,122".to_string(),
        }
    );
    assert!(matches!(
        crate::native::feature_body_selection(
            &[94, 123],
            &bindings,
            "nx:om-object-indices#94,123".to_string(),
        ),
        BodySelection::Native(_)
    ));
    let aliases = BTreeMap::from([(94, vec![first.clone()]), (150, vec![first])]);
    assert!(matches!(
        crate::native::feature_body_selection(
            &[94, 150],
            &aliases,
            "nx:om-object-indices#94,150".to_string(),
        ),
        BodySelection::Native(_)
    ));
    assert_eq!(
        crate::native::feature_body_outputs(94, &bindings),
        vec![BodyId("nx:s2:body#3".to_string())]
    );
    assert!(crate::native::feature_body_outputs(123, &bindings).is_empty());
}

#[test]
fn nx_sew_projects_ordered_body_operands_without_inventing_tolerance() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition};
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let operand = |ordinal, object_index| crate::native::FeatureOperationBodyOperand {
        id: format!("operand#{ordinal}"),
        operation_label: "operation#0".to_string(),
        body_object_index: 10,
        body_reference_ordinal: 0,
        ordinal,
        operand_object_index: object_index,
        raw_operand_object_index: vec![object_index as u8],
        segment_body_bindings: vec![format!("binding#{ordinal}")],
        source_offset: u64::from(ordinal),
    };
    let operands = [operand(0, 20), operand(1, 30)];
    let references = operands.iter().collect::<Vec<_>>();
    let primary = BodyId("body#10".to_string());
    let first = BodyId("body#20".to_string());
    let second = BodyId("body#30".to_string());
    let bodies = BTreeMap::from([
        (10, vec![primary.clone()]),
        (20, vec![first.clone()]),
        (30, vec![second.clone()]),
    ]);

    assert_eq!(
        crate::native::sew_body_feature_definition(10, &references, &bodies),
        Some(FeatureDefinition::SewBodies {
            bodies: BodySelection::Resolved {
                bodies: vec![primary.clone(), first, second],
                native: "nx:om-object-indices#10,20,30".to_string(),
            },
            gap_tolerance: None,
        })
    );
    assert_eq!(
        crate::native::sew_body_feature_definition(10, &[], &bodies),
        None
    );

    let aliased = BodyId("body#alias".to_string());
    let alias_bodies = BTreeMap::from([
        (10, vec![primary.clone()]),
        (20, vec![aliased.clone()]),
        (30, vec![aliased]),
    ]);
    assert!(matches!(
        crate::native::sew_body_feature_definition(10, &references, &alias_bodies),
        Some(FeatureDefinition::SewBodies {
            bodies: BodySelection::Native(native),
            ..
        }) if native == "nx:om-object-indices#10,20,30"
    ));
}

#[test]
fn nx_delete_body_requires_a_primary_body_field() {
    use cadmpeg_ir::features::{BodyRetentionMode, BodySelection, FeatureDefinition};
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let body = BodyId("body#20".to_string());
    let bodies = BTreeMap::from([(20, vec![body.clone()])]);
    assert_eq!(
        crate::native::delete_body_feature_definition(Some(20), &bodies),
        Some(FeatureDefinition::DeleteBody {
            bodies: BodySelection::Resolved {
                bodies: vec![body],
                native: "nx:om-object-index#20".to_string(),
            },
            mode: BodyRetentionMode::DeleteSelected,
        })
    );
    assert_eq!(
        crate::native::delete_body_feature_definition(None, &bodies),
        None
    );
}

#[test]
fn nx_trim_body_projects_distinct_target_and_ordered_tools() {
    use cadmpeg_ir::features::{BodySelection, BodyTrimSide, FeatureDefinition};
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let operands = [crate::native::FeatureOperationBodyOperand {
        id: "operand#0".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 10,
        body_reference_ordinal: 0,
        ordinal: 0,
        operand_object_index: 20,
        raw_operand_object_index: vec![20],
        segment_body_bindings: vec!["binding#0".to_string()],
        source_offset: 0,
    }];
    let references = operands.iter().collect::<Vec<_>>();
    let target = BodyId("body#10".to_string());
    let tool = BodyId("body#20".to_string());
    let bodies = BTreeMap::from([(10, vec![target.clone()]), (20, vec![tool.clone()])]);

    assert_eq!(
        crate::native::trim_body_feature_definition(10, &references, &bodies),
        Some(FeatureDefinition::TrimBodies {
            targets: BodySelection::Resolved {
                bodies: vec![target],
                native: "nx:om-object-index#10".to_string(),
            },
            tools: BodySelection::Resolved {
                bodies: vec![tool],
                native: "nx:om-object-indices#20".to_string(),
            },
            keep: BodyTrimSide::Unresolved,
        })
    );
    assert_eq!(
        crate::native::trim_body_feature_definition(10, &[], &bodies),
        None
    );

    let aliased_body = BodyId("body#alias".to_string());
    let same_body = BTreeMap::from([(10, vec![aliased_body.clone()]), (20, vec![aliased_body])]);
    assert!(matches!(
        crate::native::trim_body_feature_definition(10, &references, &same_body),
        Some(FeatureDefinition::TrimBodies {
            targets: BodySelection::Native(target),
            tools: BodySelection::Native(tools),
            ..
        }) if target == "nx:om-object-index#10" && tools == "nx:om-object-indices#20"
    ));
}

#[test]
fn nx_boolean_projection_rejects_target_tool_alias_overlap() {
    use cadmpeg_ir::features::{BodySelection, BooleanOp, FeatureDefinition};
    use cadmpeg_ir::ids::BodyId;
    use std::collections::BTreeMap;

    let operation = crate::native::FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#0".to_string(),
        kind: crate::native::FeatureBooleanKind::Subtract,
        target_object_index: 10,
        raw_target_object_index: vec![10],
        target_source_offset: 0,
        tool_object_indices: vec![20],
        raw_tool_object_indices: vec![vec![20]],
        tool_source_offsets: vec![1],
        source_offset: 0,
    };
    let body = BodyId("body#10".to_string());
    let bodies = BTreeMap::from([(10, vec![body.clone()]), (20, vec![body])]);

    assert_eq!(
        crate::native::boolean_feature_definition(&operation, &bodies),
        FeatureDefinition::Combine {
            target: BodySelection::Native("nx:om-object-index#10".to_string()),
            tools: BodySelection::Native("nx:om-object-indices#20".to_string()),
            op: BooleanOp::Cut,
        }
    );

    let missing_tool = BTreeMap::from([(10, vec![BodyId("body#10".to_string())])]);
    assert!(matches!(
        crate::native::boolean_feature_definition(&operation, &missing_tool),
        FeatureDefinition::Combine {
            target: BodySelection::Native(target),
            tools: BodySelection::Native(tools),
            ..
        } if target == "nx:om-object-index#10" && tools == "nx:om-object-indices#20"
    ));
}

#[test]
fn nx_named_operation_families_preserve_unresolved_semantics() {
    assert!(matches!(
        crate::native::non_boolean_feature_definition("SKETCH", &[], None, None, None),
        cadmpeg_ir::features::FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Unresolved,
            sketch: None,
        }
    ));
    assert!(matches!(
        crate::native::non_boolean_feature_definition(
            "SIMPLE HOLE",
            &["Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer"],
            None,
            None,
            None,
        ),
        cadmpeg_ir::features::FeatureDefinition::Hole {
            face: None,
            position: None,
            direction: None,
            kind: cadmpeg_ir::features::HoleKind::Unresolved {
                form: Some(cadmpeg_ir::features::HoleForm::Chamfer),
                counterbore_diameter: None,
                counterbore_depth: None,
                countersink_diameter: None,
                countersink_angle: None,
            },
            exit_kind: Some(cadmpeg_ir::features::HoleKind::Unresolved {
                form: Some(cadmpeg_ir::features::HoleForm::Chamfer),
                counterbore_diameter: None,
                counterbore_depth: None,
                countersink_diameter: None,
                countersink_angle: None,
            }),
            diameter: None,
            extent: Some(cadmpeg_ir::features::Extent::ThroughAll),
            ..
        }
    ));
    assert!(matches!(
        crate::native::non_boolean_feature_definition(
            "SIMPLE HOLE",
            &["unrelated"],
            None,
            None,
            None,
        ),
        cadmpeg_ir::features::FeatureDefinition::Hole { extent: None, .. }
    ));
    for competing in [
        "Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer",
        "Hole_Unknown",
    ] {
        assert!(matches!(
            crate::native::non_boolean_feature_definition(
                "SIMPLE HOLE",
                &[
                    "Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer",
                    competing,
                ],
                None,
                None,
                None,
            ),
            cadmpeg_ir::features::FeatureDefinition::Hole {
                kind: cadmpeg_ir::features::HoleKind::Simple,
                exit_kind: None,
                extent: None,
                ..
            }
        ));
    }
    assert!(matches!(
        crate::native::non_boolean_feature_definition("DATUM_PLANE", &[], None, None, None),
        cadmpeg_ir::features::FeatureDefinition::DatumPlaneUnresolved
    ));
    assert!(matches!(
        crate::native::non_boolean_feature_definition("DATUM_CSYS", &[], None, None, None),
        cadmpeg_ir::features::FeatureDefinition::DatumCoordinateSystemUnresolved
    ));
    assert!(matches!(
        crate::native::non_boolean_feature_definition(
            "TEXT",
            &["annotation", "Arial"],
            None,
            None,
            None,
        ),
        cadmpeg_ir::features::FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::Annotations,
            ref children,
            active_child: None,
        } if children.is_empty()
    ));
    assert!(matches!(
        crate::native::non_boolean_feature_definition("TEXT", &["annotation"], None, None, None),
        cadmpeg_ir::features::FeatureDefinition::Native { .. }
    ));
    assert!(matches!(
        crate::native::non_boolean_feature_definition("TEXT", &["", ""], None, None, None),
        cadmpeg_ir::features::FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::Annotations,
            ..
        }
    ));
    assert!(matches!(
        crate::native::non_boolean_feature_definition(
            "BLOCK",
            &[],
            Some([10.0, 20.0, 30.0]),
            None,
            None,
        ),
        cadmpeg_ir::features::FeatureDefinition::Block {
            dimensions: Some([
                cadmpeg_ir::features::Length(10.0),
                cadmpeg_ir::features::Length(20.0),
                cadmpeg_ir::features::Length(30.0),
            ]),
            placement: None,
        }
    ));
    assert_eq!(
        crate::native::non_boolean_feature_definition("BLOCK", &[], None, None, None),
        cadmpeg_ir::features::FeatureDefinition::Block {
            dimensions: None,
            placement: None,
        }
    );
}

#[test]
fn nx_text_payload_projects_semantic_text_and_font_family() {
    let feature = cadmpeg_ir::features::FeatureId("feature#text".to_string());
    let annotation = crate::native::text_semantic_annotation(
        "TEXT",
        &feature,
        "nx:text#1",
        7,
        &["plate label", "Arial"],
    )
    .unwrap();
    assert_eq!(annotation.object, feature.0);
    assert_eq!(
        annotation.kind,
        cadmpeg_ir::semantic_annotations::SemanticAnnotationKind::Text
    );
    assert_eq!(annotation.text, ["plate label"]);
    assert_eq!(annotation.parameters["font_family"], "Arial");
    assert_eq!(annotation.native_ref, "nx:text#1");
    assert_eq!(annotation.order, 7);

    let empty =
        crate::native::text_semantic_annotation("TEXT", &feature, "nx:text#empty", 8, &["", ""])
            .unwrap();
    assert_eq!(empty.text, [""]);
    assert_eq!(empty.parameters["font_family"], "");

    assert!(crate::native::text_semantic_annotation(
        "BLOCK",
        &feature,
        "nx:block#1",
        0,
        &["10", "20"],
    )
    .is_none());
    assert!(crate::native::text_semantic_annotation(
        "TEXT",
        &feature,
        "nx:text#2",
        0,
        &["ambiguous", "Arial", "extra"],
    )
    .is_none());
}

#[test]
fn nx_mainstream_operation_labels_project_typed_unresolved_definitions() {
    use cadmpeg_ir::features::{
        BodySelection, BodyTrimSide, BooleanOp, ChamferSpec, EdgeSelection, FaceSelection,
        FeatureDefinition, HoleKind, PatternKind, RadiusSpec, RibDraft,
    };

    for (kind, op) in [
        ("UNITE", BooleanOp::Join),
        ("SUBTRACT", BooleanOp::Cut),
        ("INTERSECT", BooleanOp::Intersect),
    ] {
        assert_eq!(
            crate::native::non_boolean_feature_definition(kind, &[], None, None, None),
            FeatureDefinition::Combine {
                target: BodySelection::Unresolved,
                tools: BodySelection::Unresolved,
                op,
            }
        );
    }

    assert_eq!(
        crate::native::non_boolean_feature_definition("EXTRACT_BODY", &[], None, None, None),
        FeatureDefinition::ExtractBody {
            source: BodySelection::Unresolved,
        }
    );
    assert_eq!(
        crate::native::non_boolean_feature_definition("SKIN", &[], None, None, None),
        FeatureDefinition::LoftUnresolved
    );
    assert_eq!(
        crate::native::non_boolean_feature_definition("Studio Surface", &[], None, None, None),
        FeatureDefinition::FreeformSurfaceUnresolved
    );
    assert_eq!(
        crate::native::non_boolean_feature_definition("POINT", &[], None, None, None),
        FeatureDefinition::DatumPointUnresolved
    );
    assert_eq!(
        crate::native::non_boolean_feature_definition("DRAFT", &[], None, None, None),
        FeatureDefinition::DraftUnresolved
    );

    assert!(matches!(
        crate::native::non_boolean_feature_definition("HOLE PACKAGE", &[], None, None, None),
        FeatureDefinition::Hole {
            kind: HoleKind::Unresolved { form: None, .. },
            ..
        }
    ));
    assert!(matches!(
        crate::native::non_boolean_feature_definition(
            "HOLE PACKAGE",
            &[],
            None,
            None,
            Some(cadmpeg_ir::features::Length(8.0)),
        ),
        FeatureDefinition::Hole {
            diameter: Some(cadmpeg_ir::features::Length(8.0)),
            kind: HoleKind::Unresolved { form: None, .. },
            ..
        }
    ));
    assert!(matches!(
        crate::native::non_boolean_feature_definition("RIB", &[], None, None, None),
        FeatureDefinition::Rib {
            construction: cadmpeg_ir::features::RibConstruction {
                draft: RibDraft::Unresolved,
                ..
            },
            op: BooleanOp::Unresolved,
        }
    ));
    assert_eq!(
        crate::native::non_boolean_feature_definition("BLEND", &[], None, None, None),
        FeatureDefinition::Fillet {
            edges: EdgeSelection::Unresolved,
            radius: RadiusSpec::Unresolved { form: None },
        }
    );
    assert_eq!(
        crate::native::non_boolean_feature_definition("FACE_BLEND", &[], None, None, None),
        FeatureDefinition::FaceBlend {
            first_faces: FaceSelection::Unresolved,
            second_faces: FaceSelection::Unresolved,
            radius: RadiusSpec::Unresolved { form: None },
        }
    );
    for kind in ["CPROJ", "CPROJ_CMB"] {
        assert_eq!(
            crate::native::non_boolean_feature_definition(kind, &[], None, None, None),
            FeatureDefinition::ProjectedCurve {
                source: cadmpeg_ir::features::PathRef::Unresolved,
                target_faces: FaceSelection::Unresolved,
                direction: cadmpeg_ir::features::CurveProjectionDirection::State(
                    cadmpeg_ir::features::CurveProjectionDirectionState::Unresolved,
                ),
                bidirectional: None,
            }
        );
    }
    assert_eq!(
        crate::native::non_boolean_feature_definition("TRIMMED_SH", &[], None, None, None),
        FeatureDefinition::TrimSurface {
            faces: FaceSelection::Unresolved,
            tool: cadmpeg_ir::features::PathRef::Unresolved,
            keep: cadmpeg_ir::features::TrimRegion::Unresolved,
        }
    );
    assert_eq!(
        crate::native::non_boolean_feature_definition("EXTEND_SHEET", &[], None, None, None),
        FeatureDefinition::ExtendSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
            method: cadmpeg_ir::features::SurfaceExtension::Unresolved,
        }
    );
    assert!(matches!(
        crate::native::non_boolean_feature_definition("CHAMFER", &[], None, None, None),
        FeatureDefinition::Chamfer {
            edges: EdgeSelection::Unresolved,
            spec: ChamferSpec::Unresolved { form: None },
            flip_direction: None,
        }
    ));
    assert_eq!(
        crate::native::non_boolean_feature_definition("SEW", &[], None, None, None),
        FeatureDefinition::SewBodies {
            bodies: BodySelection::Unresolved,
            gap_tolerance: None,
        }
    );
    assert_eq!(
        crate::native::non_boolean_feature_definition("TRIM BODY", &[], None, None, None),
        FeatureDefinition::TrimBodies {
            targets: BodySelection::Unresolved,
            tools: BodySelection::Unresolved,
            keep: BodyTrimSide::Unresolved,
        }
    );
    assert_eq!(
        crate::native::non_boolean_feature_definition("EXTRUDE", &[], None, None, None),
        FeatureDefinition::Extrude {
            profile: cadmpeg_ir::features::ProfileRef::Unresolved,
            direction: None,
            extent: cadmpeg_ir::features::Extent::Unresolved,
            op: BooleanOp::Unresolved,
            draft: None,
            reverse_draft: None,
            direction_source: None,
            solid: None,
            face_maker: None,
            inner_wire_taper: None,
            first_offset: None,
            second_offset: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        }
    );
    assert_eq!(
        crate::native::non_boolean_feature_definition("OFFSET", &[], None, None, None),
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Unresolved,
            distance: None,
        }
    );
    assert!(matches!(
        crate::native::non_boolean_feature_definition("THICKEN_SHEET", &[], None, None, None),
        FeatureDefinition::Thicken {
            faces: FaceSelection::Unresolved,
            thickness: None,
            side: None,
        }
    ));
    for kind in ["Pattern Feature", "Pattern Geometry", "Geometry Instance"] {
        assert!(matches!(
            crate::native::non_boolean_feature_definition(kind, &[], None, None, None),
            FeatureDefinition::Pattern {
                seeds,
                pattern: PatternKind::Unresolved { form: None },
            } if seeds.is_empty()
        ));
    }
}

#[test]
fn nx_container_record_is_not_a_modeling_feature() {
    assert!(!crate::native::projects_neutral_feature("Container"));
    assert!(crate::native::projects_neutral_feature("EXTRUDE"));
}

#[test]
fn nx_pattern_completeness_requires_distinct_seeds() {
    let seed = cadmpeg_ir::features::FeatureId("test:feature#seed".into());
    let pattern = cadmpeg_ir::features::PatternKind::Mirror {
        plane_origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        plane_normal: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
    };

    assert!(!crate::decode::pattern_feature_is_incomplete(
        std::slice::from_ref(&seed),
        &pattern,
    ));
    assert!(crate::decode::pattern_feature_is_incomplete(
        &[seed.clone(), seed],
        &pattern,
    ));
}

#[test]
fn nx_face_blend_completeness_requires_disjoint_supports() {
    use cadmpeg_ir::features::FaceSelection;
    use cadmpeg_ir::ids::FaceId;

    let shared = FaceId("test:face#shared".into());
    let distinct = FaceId("test:face#distinct".into());
    let first = FaceSelection::Faces(vec![shared.clone()]);

    assert!(crate::decode::face_selections_overlap(
        &first,
        &FaceSelection::Resolved {
            faces: vec![shared],
            native: "test:first-support".into(),
        },
    ));
    assert!(!crate::decode::face_selections_overlap(
        &first,
        &FaceSelection::Faces(vec![distinct]),
    ));
    assert!(!crate::decode::face_selections_overlap(
        &first,
        &FaceSelection::Unresolved,
    ));
}

#[test]
fn nx_selection_completeness_rejects_repeated_faces_and_edges() {
    use cadmpeg_ir::features::{EdgeSelection, FaceSelection, ProfileRef};
    use cadmpeg_ir::ids::{EdgeId, FaceId};

    let face = FaceId("test:face#repeated".into());
    assert!(crate::decode::face_selection_is_incomplete(
        &FaceSelection::Faces(vec![face.clone(), face]),
    ));

    let face = FaceId("test:profile-face#repeated".into());
    assert!(crate::decode::profile_ref_is_incomplete(
        &ProfileRef::Faces(vec![face.clone(), face]),
    ));

    let edge = EdgeId("test:edge#repeated".into());
    assert!(crate::decode::edge_selection_is_incomplete(
        &EdgeSelection::Edges(vec![edge.clone(), edge]),
    ));
}

#[test]
fn nx_hole_completeness_rejects_opaque_supplied_operands() {
    use cadmpeg_ir::features::{Extent, FaceSelection, HoleKind, Length, ProfileRef};
    use cadmpeg_ir::math::{Point3, Vector3};

    let incomplete = |profile, face| {
        crate::decode::hole_feature_is_incomplete(
            profile,
            face,
            Some(Point3::new(0.0, 0.0, 0.0)),
            Some(Vector3::new(0.0, 0.0, 1.0)),
            (&HoleKind::Simple, None),
            Some(Length(1.0)),
            Some(&Extent::ThroughAll),
        )
    };

    assert!(!incomplete(None, None));
    assert!(incomplete(Some(&ProfileRef::Unresolved), None));
    assert!(incomplete(None, Some(&FaceSelection::Unresolved)));
}

#[test]
fn nx_sketch_completeness_reports_native_geometry_and_constraints() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, SketchSpace};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
        SketchEntityId, SketchGeometry, SketchId,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let sketch_id = SketchId("test:sketch#0".into());
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#sketch".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: SketchSpace::Planar,
            sketch: Some(sketch_id.clone()),
        },
        native_ref: None,
    });
    ir.model.sketches.push(Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        profiles: Vec::new(),
        native_ref: None,
    });
    let entity_id = SketchEntityId("test:sketch-entity#0".into());
    ir.model.sketch_entities.push(SketchEntity {
        id: entity_id.clone(),
        sketch: sketch_id.clone(),
        construction: false,
        native_ref: None,
        geometry_ref: None,
        endpoint_refs: Vec::new(),
        geometry: SketchGeometry::Native {
            native_kind: "test".into(),
        },
    });
    ir.model.sketch_constraints.push(SketchConstraint {
        id: SketchConstraintId("test:sketch-constraint#0".into()),
        sketch: sketch_id,
        definition: SketchConstraintDefinition::Native {
            native_kind: "test".into(),
            entities: vec![entity_id],
            parameter: None,
            operands: Vec::new(),
        },
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    });

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0]
        .message
        .contains("1 NX sketch geometry record(s) and 1 sketch constraint"));
}

#[test]
fn nx_sketch_completeness_requires_planar_space() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, SketchSpace};
    use cadmpeg_ir::sketches::SketchId;

    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#sketch".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: SketchSpace::Spatial,
            sketch: Some(SketchId("test:sketch#0".into())),
        },
        native_ref: None,
    });

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.iter().any(|loss| {
        loss.message.contains(
            "construction fields or output lineage remain unresolved or native-only: sketch (1)",
        )
    }));
}

#[test]
fn nx_body_operation_completeness_requires_disjoint_roles() {
    use cadmpeg_ir::features::BodySelection;
    use cadmpeg_ir::ids::BodyId;

    let shared = BodyId("test:body#shared".into());
    let distinct = BodyId("test:body#distinct".into());
    let target = BodySelection::Bodies(vec![shared.clone()]);

    assert!(crate::decode::body_selection_is_incomplete(
        &BodySelection::Bodies(vec![shared.clone(), shared.clone()]),
    ));
    assert!(!crate::decode::body_selection_is_incomplete(&target));

    assert!(crate::decode::body_selections_overlap(
        &target,
        &BodySelection::Resolved {
            bodies: vec![shared],
            native: "test:tools".into(),
        },
    ));
    assert!(!crate::decode::body_selections_overlap(
        &target,
        &BodySelection::Bodies(vec![distinct]),
    ));
    assert!(!crate::decode::body_selections_overlap(
        &target,
        &BodySelection::Unresolved,
    ));
}

#[test]
fn nx_configuration_completeness_requires_one_active_full_body_set() {
    use cadmpeg_ir::features::{ConfigurationBodies, ConfigurationId, DesignConfiguration};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let bodies = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId("test:configuration#0".into()),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: "Model".into(),
        material: None,
        properties: Default::default(),
        bodies: ConfigurationBodies::Resolved(Vec::new()),
        native_ref: None,
    });

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1 NX design configuration"));

    ir.model.configurations[0].bodies = ConfigurationBodies::Resolved(bodies);
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.configurations[0].active = false;
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("1 NX design configuration"));
}

#[test]
fn nx_body_producing_feature_families_require_history_outputs() {
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, Length};
    use std::collections::BTreeMap;

    let mut ir = cadmpeg_ir::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#block".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Block {
            dimensions: Some([Length(1.0), Length(2.0), Length(3.0)]),
            placement: Some(cadmpeg_ir::transform::Transform::identity()),
        },
        native_ref: None,
    });

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("block (1)"));

    let output = cadmpeg_ir::ids::BodyId("test:body#output".into());
    ir.model.features[0].outputs = vec![output.clone()];
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("block (1)"));

    ir.model.features[0].outputs = vec![output.clone(), output.clone()];
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("block (1)"));

    ir.model.features[0].suppressed = Some(true);
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.features[0].definition = FeatureDefinition::Loft {
        profiles: Vec::new(),
        guides: Vec::new(),
        op: cadmpeg_ir::features::BooleanOp::Unresolved,
        closed: false,
        solid: false,
        ruled: false,
        max_degree: None,
        check_compatibility: None,
        allow_multi_profile_faces: None,
    };
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("loft (1)"));

    ir.model.features[0].definition = FeatureDefinition::Draft {
        faces: cadmpeg_ir::features::FaceSelection::Unresolved,
        neutral_plane: cadmpeg_ir::features::FaceSelection::Unresolved,
        pull_direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        angle: cadmpeg_ir::features::Angle(0.1),
        outward: false,
    };
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("draft (1)"));

    ir.model.features[0].definition = FeatureDefinition::DatumOffsetPlane {
        reference: None,
        distance: Length(5.0),
    };
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("datum plane (1)"));

    let datum = FeatureId("test:feature#datum-source".into());
    ir.model.features[0].definition = FeatureDefinition::DatumOffsetPlane {
        reference: Some(datum.clone()),
        distance: Length(5.0),
    };
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("datum plane (1)"));

    ir.model.features[0].ordinal = 1;
    ir.model.features.push(Feature {
        id: datum.clone(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DatumPrincipalPlane {
            plane: cadmpeg_ir::features::PrincipalPlane::Top,
        },
        native_ref: None,
    });
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("datum plane (1)"));

    ir.model.features[0].dependencies.push(datum);
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());

    ir.model.features[0].definition = FeatureDefinition::SewBodies {
        bodies: cadmpeg_ir::features::BodySelection::Bodies(vec![output.clone()]),
        gap_tolerance: Some(Length(0.01)),
    };
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert_eq!(losses.len(), 1);
    assert!(losses[0].message.contains("sew bodies (1)"));

    assert_eq!(
        crate::decode::body_output_feature_family(&FeatureDefinition::DatumPointUnresolved),
        None
    );
    assert_eq!(
        crate::decode::body_output_feature_family(&FeatureDefinition::Loft {
            profiles: Vec::new(),
            guides: Vec::new(),
            op: cadmpeg_ir::features::BooleanOp::NewBody,
            closed: false,
            solid: false,
            ruled: false,
            max_degree: None,
            check_compatibility: None,
            allow_multi_profile_faces: None,
        }),
        Some("loft")
    );
    assert_eq!(
        crate::decode::body_output_feature_family(&FeatureDefinition::Draft {
            faces: cadmpeg_ir::features::FaceSelection::Unresolved,
            neutral_plane: cadmpeg_ir::features::FaceSelection::Unresolved,
            pull_direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            angle: cadmpeg_ir::features::Angle(0.1),
            outward: false,
        }),
        Some("draft")
    );
    assert_eq!(
        crate::decode::body_output_feature_family(&FeatureDefinition::DeleteBody {
            bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
        }),
        None
    );
}

#[test]
fn nx_sew_completeness_does_not_invent_a_gap_tolerance() {
    use cadmpeg_ir::features::{BodySelection, Feature, FeatureDefinition, FeatureId};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let first = ir.model.bodies[0].id.clone();
    let mut second_body = ir.model.bodies[0].clone();
    second_body.id = cadmpeg_ir::ids::BodyId("test:body#second".into());
    let second = second_body.id.clone();
    ir.model.bodies.push(second_body);
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#sew".into()),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: vec![first.clone()],
        definition: FeatureDefinition::SewBodies {
            bodies: BodySelection::Bodies(vec![first, second]),
            gap_tolerance: None,
        },
        native_ref: None,
    });

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);
    assert!(losses.is_empty());
}

#[test]
fn nx_block_placement_requires_native_dimensions_and_unique_axes() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let dimensions = [10.0, 20.0, 30.0];
    for axis in 0..3 {
        let mut surfaces = ir
            .model
            .surfaces
            .iter_mut()
            .filter_map(|surface| {
                let SurfaceGeometry::Plane { origin, normal, .. } = &mut surface.geometry else {
                    return None;
                };
                let components = [normal.x.abs(), normal.y.abs(), normal.z.abs()];
                (components[axis] > 0.5).then_some(origin)
            })
            .collect::<Vec<_>>();
        assert_eq!(surfaces.len(), 2);
        surfaces.sort_by(|first, second| {
            [first.x, first.y, first.z][axis].total_cmp(&[second.x, second.y, second.z][axis])
        });
        match axis {
            0 => {
                surfaces[0].x = 0.0;
                surfaces[1].x = dimensions[axis];
            }
            1 => {
                surfaces[0].y = 0.0;
                surfaces[1].y = dimensions[axis];
            }
            2 => {
                surfaces[0].z = 0.0;
                surfaces[1].z = dimensions[axis];
            }
            _ => unreachable!(),
        }
    }
    let output = ir.model.bodies[0].id.clone();

    assert_eq!(
        crate::native::block_placement(&ir, dimensions, std::slice::from_ref(&output)),
        Some(cadmpeg_ir::transform::Transform::identity())
    );
    assert_eq!(
        crate::native::block_placement(&ir, dimensions, &[]),
        Some(cadmpeg_ir::transform::Transform::identity())
    );
    assert_eq!(
        crate::native::block_placement(&ir, dimensions, &[output.clone(), output.clone()],),
        None
    );
    assert_eq!(
        crate::native::block_placement(&ir, [10.0, 10.0, 30.0], std::slice::from_ref(&output),),
        None
    );

    let mut repeated = ir.clone();
    let high_y = repeated
        .model
        .surfaces
        .iter_mut()
        .filter_map(|surface| {
            let SurfaceGeometry::Plane { origin, normal, .. } = &mut surface.geometry else {
                return None;
            };
            (normal.y.abs() > 0.5 && origin.y > 0.0).then_some(origin)
        })
        .next()
        .expect("positive y plane");
    high_y.y = 10.0;
    assert_eq!(
        crate::native::block_placement(
            &repeated,
            [10.0, 10.0, 30.0],
            std::slice::from_ref(&output),
        ),
        None
    );

    let mut stepped = ir.clone();
    let mut intermediate_surface = stepped
        .model
        .surfaces
        .iter()
        .find(|surface| {
            matches!(
                &surface.geometry,
                SurfaceGeometry::Plane { normal, .. } if normal.x.abs() > 0.5
            )
        })
        .expect("x-normal plane")
        .clone();
    intermediate_surface.id = cadmpeg_ir::ids::SurfaceId("intermediate-plane".into());
    let SurfaceGeometry::Plane { origin, .. } = &mut intermediate_surface.geometry else {
        unreachable!()
    };
    origin.x = 5.0;
    stepped.model.surfaces.push(intermediate_surface);
    let mut intermediate_face = stepped.model.faces.first().expect("cube face").clone();
    intermediate_face.id = cadmpeg_ir::ids::FaceId("intermediate-face".into());
    intermediate_face.surface = cadmpeg_ir::ids::SurfaceId("intermediate-plane".into());
    intermediate_face.loops.clear();
    stepped.model.shells[0]
        .faces
        .push(intermediate_face.id.clone());
    stepped.model.faces.push(intermediate_face);
    assert_eq!(
        crate::native::block_placement(&stepped, dimensions, std::slice::from_ref(&output)),
        None
    );

    let mut nonplanar = ir.clone();
    nonplanar.model.surfaces[0].geometry = SurfaceGeometry::Sphere {
        center: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
    };
    assert_eq!(
        crate::native::block_placement(&nonplanar, dimensions, std::slice::from_ref(&output)),
        None
    );

    let mut missing_surface = ir.clone();
    let removed = missing_surface.model.surfaces.pop().expect("cube surface");
    assert!(missing_surface
        .model
        .faces
        .iter()
        .any(|face| face.surface == removed.id));
    assert_eq!(
        crate::native::block_placement(&missing_surface, dimensions, &[]),
        None
    );

    let mut curved_feature = ir.clone();
    let mut curved_surface = curved_feature.model.surfaces[0].clone();
    curved_surface.id = cadmpeg_ir::ids::SurfaceId("later-curved-surface".into());
    curved_surface.geometry = SurfaceGeometry::Sphere {
        center: cadmpeg_ir::math::Point3::new(5.0, 10.0, 15.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
    };
    curved_feature.model.surfaces.push(curved_surface);
    let mut curved_face = curved_feature.model.faces[0].clone();
    curved_face.id = cadmpeg_ir::ids::FaceId("later-curved-face".into());
    curved_face.surface = cadmpeg_ir::ids::SurfaceId("later-curved-surface".into());
    curved_face.loops.clear();
    curved_feature.model.shells[0]
        .faces
        .push(curved_face.id.clone());
    curved_feature.model.faces.push(curved_face);
    assert_eq!(
        crate::native::block_placement(&curved_feature, dimensions, &[]),
        Some(cadmpeg_ir::transform::Transform::identity())
    );

    let mut sheet = ir.clone();
    sheet.model.bodies[0].kind = cadmpeg_ir::topology::BodyKind::Sheet;
    assert_eq!(
        crate::native::block_placement(&sheet, dimensions, std::slice::from_ref(&output)),
        None
    );

    let mut disconnected = ir.clone();
    let mut second_region = disconnected.model.regions[0].clone();
    second_region.id = cadmpeg_ir::ids::RegionId("second-region".into());
    second_region.shells.clear();
    disconnected.model.bodies[0]
        .regions
        .push(second_region.id.clone());
    disconnected.model.regions.push(second_region);
    assert_eq!(
        crate::native::block_placement(&disconnected, dimensions, std::slice::from_ref(&output)),
        None
    );
}

#[test]
fn nx_simple_hole_template_requires_exact_ordered_tokens() {
    use crate::native::{
        FeatureOperationLabel, FeatureOperationRecord, FeaturePayloadString,
        SimpleHoleEndTreatment, SimpleHoleExtent, SimpleHoleFamily, SimpleHoleForm,
    };

    let label = FeatureOperationLabel {
        id: "operation#3".to_string(),
        section_link: "section#0".to_string(),
        ordinal: 3,
        value: "SIMPLE HOLE".to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        source_offset: 100,
    };
    let record = FeatureOperationRecord {
        id: "record#3".to_string(),
        operation_label: label.id.clone(),
        ordinal: 3,
        byte_len: 80,
        sha256: "a".repeat(64),
        payload_byte_len: 40,
        payload_sha256: "b".repeat(64),
        payload_source_offset: 120,
        source_offset: 90,
    };
    let string = FeaturePayloadString {
        id: "payload-string#3-0".to_string(),
        operation_record: record.id.clone(),
        ordinal: 0,
        value: "Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer".to_string(),
        source_offset: 130,
    };
    let templates = crate::native::feature_simple_hole_templates(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        std::slice::from_ref(&string),
    );
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].payload_string, string.id);
    assert_eq!(templates[0].family, SimpleHoleFamily::GeneralHole);
    assert_eq!(templates[0].form, SimpleHoleForm::Simple);
    assert_eq!(templates[0].extent, SimpleHoleExtent::Through);
    assert_eq!(
        templates[0].start_treatment,
        SimpleHoleEndTreatment::Chamfer
    );
    assert_eq!(templates[0].end_treatment, SimpleHoleEndTreatment::Chamfer);

    let mut duplicate = string.clone();
    duplicate.id = "payload-string#3-1".to_string();
    duplicate.ordinal = 1;
    duplicate.source_offset += 64;
    assert!(crate::native::feature_simple_hole_templates(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        &[string.clone(), duplicate],
    )
    .is_empty());

    let unknown = FeaturePayloadString {
        id: "payload-string#3-1".to_string(),
        operation_record: record.id.clone(),
        ordinal: 1,
        value: "Hole_Unknown".to_string(),
        source_offset: 194,
    };
    assert!(crate::native::feature_simple_hole_templates(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        &[string.clone(), unknown],
    )
    .is_empty());

    let mut malformed = string;
    malformed.value = "Hole_GeneralHole_Simple_Through_EndChamfer_StartChamfer".to_string();
    assert!(
        crate::native::feature_simple_hole_templates(&[label], &[record], &[malformed]).is_empty()
    );
}

#[test]
fn nx_simple_hole_feature_owns_its_exact_native_constructions() {
    use crate::native::{
        FeatureSimpleHoleConstructionGroup, FeatureSimpleHoleRepeatedScalarLane,
        FeatureSimpleHoleRepeatedScalarLaneBlockReferences, FeatureSimpleHoleTemplate,
        SimpleHoleEndTreatment, SimpleHoleExtent, SimpleHoleFamily, SimpleHoleForm,
    };
    let operation = "nx:feature-history:operation-label#1-4";
    let template = FeatureSimpleHoleTemplate {
        id: "template".to_string(),
        operation_label: operation.to_string(),
        payload_string: "string".to_string(),
        family: SimpleHoleFamily::GeneralHole,
        form: SimpleHoleForm::Simple,
        extent: SimpleHoleExtent::Through,
        start_treatment: SimpleHoleEndTreatment::Chamfer,
        end_treatment: SimpleHoleEndTreatment::Chamfer,
    };
    let lane = FeatureSimpleHoleRepeatedScalarLane {
        id: "lane".to_string(),
        operation_label: operation.to_string(),
        values: vec![508.0, 38.1],
        raw_values: vec![[0x30; 8], [0x31; 8]],
        first_witness_offsets: vec![10, 18],
        second_witness_offsets: vec![30, 38],
    };
    let blocks = FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
        id: "blocks".to_string(),
        operation_label: operation.to_string(),
        first_data_blocks: ["block#231".to_string(), "block#232".to_string()],
        second_data_blocks: ["block#233".to_string(), "block#234".to_string()],
        first_reference_offsets: [20, 22],
        second_reference_offsets: [40, 42],
    };
    let group = FeatureSimpleHoleConstructionGroup {
        id: "group".into(),
        first_data_blocks: blocks.first_data_blocks.clone(),
        second_data_blocks: blocks.second_data_blocks.clone(),
        operation_labels: vec![operation.into(), "other-operation".into()],
        scalar_lanes: vec!["lane".into(), "other-lane".into()],
        block_references: vec!["blocks".into(), "other-blocks".into()],
    };
    let properties = crate::native::simple_hole_native_properties(
        operation,
        &[template],
        &[lane],
        &[blocks],
        &[group],
    );
    assert_eq!(properties["simple_hole_template"], "template");
    assert_eq!(properties["simple_hole_repeated_scalar_lane"], "lane");
    assert_eq!(
        properties["simple_hole_repeated_scalar_lane_block_references"],
        "blocks"
    );
    assert_eq!(properties["simple_hole_construction_group"], "group");
    assert!(crate::native::simple_hole_native_properties(
        "nx:feature-history:operation-label#1-5",
        &[],
        &[],
        &[],
        &[],
    )
    .is_empty());
}

#[test]
fn nx_hole_geometry_projection_requires_complete_through_bore_partitions() {
    use crate::native::{
        FeatureSimpleHoleConstructionGroup, FeatureSimpleHoleTemplate, SimpleHoleEndTreatment,
        SimpleHoleExtent, SimpleHoleFamily, SimpleHoleForm,
    };
    use cadmpeg_ir::document::{CadIr, Model, IR_VERSION};
    use cadmpeg_ir::geometry::{Curve, CurveGeometry, Surface};
    use cadmpeg_ir::ids::{
        BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, RegionId, ShellId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::native::Native;
    use cadmpeg_ir::topology::{Body, BodyKind, Coedge, Edge, Face, Region, Sense, Shell};
    use cadmpeg_ir::units::{Tolerances, Units};
    use cadmpeg_ir::SourceObjectAssociation;

    let operations = ["hole-a".to_string(), "hole-b".to_string()];
    let templates = operations
        .iter()
        .map(|operation| FeatureSimpleHoleTemplate {
            id: format!("template-{operation}"),
            operation_label: operation.clone(),
            payload_string: format!("string-{operation}"),
            family: SimpleHoleFamily::GeneralHole,
            form: SimpleHoleForm::Simple,
            extent: SimpleHoleExtent::Through,
            start_treatment: SimpleHoleEndTreatment::Chamfer,
            end_treatment: SimpleHoleEndTreatment::Chamfer,
        })
        .collect::<Vec<_>>();
    let group = FeatureSimpleHoleConstructionGroup {
        id: "group".into(),
        first_data_blocks: ["a".into(), "b".into()],
        second_data_blocks: ["c".into(), "d".into()],
        operation_labels: operations.to_vec(),
        scalar_lanes: vec!["lane-a".into(), "lane-b".into()],
        block_references: vec!["refs-a".into(), "refs-b".into()],
    };
    let mut model = Model::default();
    for ordinal in 0..2 {
        let surface = SurfaceId(format!("surface-{ordinal}"));
        model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(ordinal as f64, 0.0, 0.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.55,
            },
            source_object: None::<SourceObjectAssociation>,
        });
        model.faces.push(Face {
            id: FaceId(format!("face-{ordinal}")),
            shell: ShellId("shell".into()),
            surface,
            sense: Sense::Reversed,
            loops: vec![
                LoopId(format!("loop-{ordinal}-0")),
                LoopId(format!("loop-{ordinal}-1")),
            ],
            name: None,
            color: None,
            tolerance: None,
        });
        for boundary in 0..2 {
            let loop_id = LoopId(format!("loop-{ordinal}-{boundary}"));
            let curve = CurveId(format!("bore-curve-{ordinal}-{boundary}"));
            let edge = EdgeId(format!("bore-edge-{ordinal}-{boundary}"));
            let coedge = CoedgeId(format!("bore-coedge-{ordinal}-{boundary}"));
            model.curves.push(Curve {
                id: curve.clone(),
                geometry: CurveGeometry::Circle {
                    center: Point3::new(ordinal as f64, boundary as f64, 0.0),
                    axis: Vector3::new(0.0, 1.0, 0.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    radius: 2.55,
                },
                source_object: None,
            });
            model.edges.push(Edge {
                id: edge.clone(),
                curve: Some(curve),
                start: VertexId("vertex".into()),
                end: VertexId("vertex".into()),
                param_range: None,
                tolerance: None,
            });
            model.coedges.push(Coedge {
                id: coedge.clone(),
                owner_loop: loop_id,
                edge,
                next: coedge.clone(),
                previous: coedge.clone(),
                radial_next: coedge,
                sense: Sense::Forward,
                pcurves: Vec::new(),
            });
        }
    }
    let body = BodyId("body".into());
    model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::Solid,
        regions: vec![RegionId("region".into())],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    model.regions.push(Region {
        id: RegionId("region".into()),
        body: body.clone(),
        shells: vec![ShellId("shell".into())],
    });
    model.shells.push(Shell {
        id: ShellId("shell".into()),
        region: RegionId("region".into()),
        faces: vec![FaceId("face-0".into()), FaceId("face-1".into())],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    let ir = CadIr {
        ir_version: IR_VERSION.into(),
        source: None,
        units: Units::default(),
        tolerances: Tolerances::default(),
        model,
        native: Native::default(),
    };
    let outputs = std::collections::BTreeMap::from([
        ("hole-a".to_string(), vec![body.clone()]),
        ("hole-b".to_string(), vec![body]),
    ]);
    assert_eq!(
        crate::native::simple_hole_diameters(
            &ir,
            &templates,
            std::slice::from_ref(&group),
            &outputs,
        ),
        std::collections::BTreeMap::from([
            ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
            ("hole-b".into(), cadmpeg_ir::features::Length(5.1)),
        ])
    );
    assert_eq!(
        crate::native::simple_hole_diameters(&ir, &templates, &[], &outputs),
        std::collections::BTreeMap::from([
            ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
            ("hole-b".into(), cadmpeg_ir::features::Length(5.1)),
        ])
    );
    assert_eq!(
        crate::native::hole_diameters_for_operations(&ir, &operations, &outputs),
        std::collections::BTreeMap::from([
            ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
            ("hole-b".into(), cadmpeg_ir::features::Length(5.1)),
        ])
    );
    let expected_directions = std::collections::BTreeMap::from([
        ("hole-a".into(), Vector3::new(0.0, 1.0, 0.0)),
        ("hole-b".into(), Vector3::new(0.0, 1.0, 0.0)),
    ]);
    assert_eq!(
        crate::native::hole_directions_for_operations(&ir, &operations, &outputs),
        expected_directions
    );
    assert_eq!(
        crate::native::hole_directions_for_operations(
            &ir,
            &operations,
            &std::collections::BTreeMap::new(),
        ),
        expected_directions
    );
    assert!(crate::native::hole_positions_for_operations(&ir, &operations, &outputs).is_empty());
    let mut single_hole = ir.clone();
    single_hole.model.shells[0].faces = vec![FaceId("face-1".into())];
    let single_operation = [operations[1].clone()];
    let single_output = std::collections::BTreeMap::from([(
        operations[1].clone(),
        outputs[&operations[1]].clone(),
    )]);
    assert_eq!(
        crate::native::hole_positions_for_operations(
            &single_hole,
            &single_operation,
            &single_output,
        ),
        std::collections::BTreeMap::from([(operations[1].clone(), Point3::new(1.0, 0.0, 0.0),)])
    );
    let SurfaceGeometry::Cylinder { origin, .. } = &mut single_hole.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    origin.y = 91.0;
    assert_eq!(
        crate::native::hole_positions_for_operations(
            &single_hole,
            &single_operation,
            &single_output,
        ),
        std::collections::BTreeMap::from([(operations[1].clone(), Point3::new(1.0, 0.0, 0.0),)])
    );
    let mut opposite_axis = ir.clone();
    let SurfaceGeometry::Cylinder { axis, .. } = &mut opposite_axis.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    *axis = Vector3::new(0.0, -1.0, 0.0);
    for curve in opposite_axis
        .model
        .curves
        .iter_mut()
        .filter(|curve| curve.id.0.starts_with("bore-curve-1-"))
    {
        let CurveGeometry::Circle { axis, .. } = &mut curve.geometry else {
            unreachable!()
        };
        *axis = Vector3::new(0.0, -1.0, 0.0);
    }
    assert_eq!(
        crate::native::hole_directions_for_operations(&opposite_axis, &operations, &outputs),
        expected_directions
    );
    let mut different_radii = ir.clone();
    let SurfaceGeometry::Cylinder { radius, .. } = &mut different_radii.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    *radius = 3.1;
    for curve in different_radii
        .model
        .curves
        .iter_mut()
        .filter(|curve| curve.id.0.starts_with("bore-curve-1-"))
    {
        let CurveGeometry::Circle { radius, .. } = &mut curve.geometry else {
            unreachable!()
        };
        *radius = 3.1;
    }
    assert!(
        crate::native::hole_diameters_for_operations(&different_radii, &operations, &outputs,)
            .is_empty()
    );
    assert_eq!(
        crate::native::hole_directions_for_operations(&different_radii, &operations, &outputs),
        expected_directions
    );
    assert_eq!(
        crate::native::simple_hole_diameters(
            &ir,
            &templates,
            std::slice::from_ref(&group),
            &std::collections::BTreeMap::new(),
        ),
        std::collections::BTreeMap::from([
            ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
            ("hole-b".into(), cadmpeg_ir::features::Length(5.1)),
        ])
    );
    assert!(crate::native::hole_diameters_for_operations(
        &ir,
        &[operations[0].clone(), operations[0].clone()],
        &outputs,
    )
    .is_empty());
    let mut invalid_boundary = ir.clone();
    let CurveGeometry::Circle { radius, .. } = &mut invalid_boundary.model.curves[0].geometry
    else {
        unreachable!()
    };
    *radius += 0.1;
    assert!(
        crate::native::hole_diameters_for_operations(&invalid_boundary, &operations, &outputs,)
            .is_empty()
    );
    let mut coincident_boundaries = ir.clone();
    let CurveGeometry::Circle { center, .. } = &mut coincident_boundaries.model.curves[1].geometry
    else {
        unreachable!()
    };
    center.y = 0.0;
    assert!(crate::native::hole_diameters_for_operations(
        &coincident_boundaries,
        &operations,
        &outputs,
    )
    .is_empty());
    let mut nonparallel = ir.clone();
    let SurfaceGeometry::Cylinder { axis, .. } = &mut nonparallel.model.surfaces[1].geometry else {
        unreachable!()
    };
    *axis = Vector3::new(0.0, 0.0, 1.0);
    assert!(
        crate::native::hole_directions_for_operations(&nonparallel, &operations, &outputs)
            .is_empty()
    );
    let mut sheet = ir.clone();
    sheet.model.bodies[0].kind = BodyKind::Sheet;
    assert!(crate::native::hole_diameters_for_operations(&sheet, &operations, &outputs).is_empty());
    let mut disconnected = ir.clone();
    disconnected.model.bodies[0]
        .regions
        .push(RegionId("second-region".into()));
    assert!(
        crate::native::hole_diameters_for_operations(&disconnected, &operations, &outputs)
            .is_empty()
    );
    let mut shared_carrier = ir.clone();
    shared_carrier.model.faces.push(Face {
        id: FaceId("unowned-shared-cylinder-face".into()),
        shell: ShellId("unowned-shell".into()),
        surface: SurfaceId("surface-0".into()),
        sense: Sense::Reversed,
        loops: vec![
            LoopId("unowned-loop-a".into()),
            LoopId("unowned-loop-b".into()),
        ],
        name: None,
        color: None,
        tolerance: None,
    });
    assert_eq!(
        crate::native::simple_hole_diameters(
            &shared_carrier,
            &templates,
            std::slice::from_ref(&group),
            &outputs,
        ),
        crate::native::simple_hole_diameters(
            &ir,
            &templates,
            std::slice::from_ref(&group),
            &outputs,
        )
    );

    let mut distinct = ir.clone();
    distinct.model.shells[0].faces.pop();
    distinct.model.bodies.push(Body {
        id: BodyId("second-body".into()),
        kind: BodyKind::Solid,
        regions: vec![RegionId("second-region".into())],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    distinct.model.regions.push(Region {
        id: RegionId("second-region".into()),
        body: BodyId("second-body".into()),
        shells: vec![ShellId("second-shell".into())],
    });
    distinct.model.shells.push(Shell {
        id: ShellId("second-shell".into()),
        region: RegionId("second-region".into()),
        faces: vec![FaceId("face-1".into())],
        wire_edges: Vec::new(),
        free_vertices: Vec::new(),
    });
    distinct.model.faces[1].shell = ShellId("second-shell".into());
    let SurfaceGeometry::Cylinder { radius, .. } = &mut distinct.model.surfaces[1].geometry else {
        unreachable!()
    };
    *radius = 3.0;
    for curve in distinct
        .model
        .curves
        .iter_mut()
        .filter(|curve| curve.id.0.starts_with("bore-curve-1-"))
    {
        let CurveGeometry::Circle { radius, .. } = &mut curve.geometry else {
            unreachable!()
        };
        *radius = 3.0;
    }
    let distinct_outputs = std::collections::BTreeMap::from([
        ("hole-a".to_string(), vec![BodyId("body".into())]),
        ("hole-b".to_string(), vec![BodyId("second-body".into())]),
    ]);
    assert_eq!(
        crate::native::simple_hole_diameters(
            &distinct,
            &templates,
            std::slice::from_ref(&group),
            &distinct_outputs,
        ),
        std::collections::BTreeMap::from([
            ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
            ("hole-b".into(), cadmpeg_ir::features::Length(6.0)),
        ])
    );
    assert_eq!(
        crate::native::hole_diameters_for_operations(&distinct, &operations, &distinct_outputs,),
        std::collections::BTreeMap::from([
            ("hole-a".into(), cadmpeg_ir::features::Length(5.1)),
            ("hole-b".into(), cadmpeg_ir::features::Length(6.0)),
        ])
    );
    assert!(crate::native::hole_diameters_for_operations(
        &distinct,
        &operations,
        &std::collections::BTreeMap::new(),
    )
    .is_empty());
    assert!(crate::native::hole_diameters_for_operations(
        &ir,
        &operations,
        &std::collections::BTreeMap::from([("hole-a".to_string(), vec![BodyId("body".into())],)]),
    )
    .is_empty());

    let mut chamfered = ir.clone();
    for bore in 0..2 {
        for end in 0..2 {
            let surface = SurfaceId(format!("cone-{bore}-{end}"));
            let face = FaceId(format!("cone-face-{bore}-{end}"));
            let loops = [
                LoopId(format!("cone-loop-{bore}-{end}-inner")),
                LoopId(format!("cone-loop-{bore}-{end}-outer")),
            ];
            chamfered.model.surfaces.push(Surface {
                id: surface.clone(),
                geometry: SurfaceGeometry::Cone {
                    origin: Point3::new(bore as f64, end as f64, 0.0),
                    axis: Vector3::new(0.0, if end == 0 { 1.0 } else { -1.0 }, 0.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    radius: 0.0,
                    ratio: 1.0,
                    half_angle: std::f64::consts::FRAC_PI_4,
                },
                source_object: None,
            });
            chamfered.model.shells[0].faces.push(face.clone());
            chamfered.model.faces.push(Face {
                id: face,
                shell: ShellId("shell".into()),
                surface,
                sense: Sense::Reversed,
                loops: loops.to_vec(),
                name: None,
                color: None,
                tolerance: None,
            });
            for (boundary, (loop_id, radius)) in loops.into_iter().zip([2.55, 3.55]).enumerate() {
                let curve = CurveId(format!("cone-curve-{bore}-{end}-{boundary}"));
                let edge = EdgeId(format!("cone-edge-{bore}-{end}-{boundary}"));
                let coedge = CoedgeId(format!("cone-coedge-{bore}-{end}-{boundary}"));
                chamfered.model.curves.push(Curve {
                    id: curve.clone(),
                    geometry: CurveGeometry::Circle {
                        center: Point3::new(bore as f64, end as f64, 0.0),
                        axis: Vector3::new(0.0, 1.0, 0.0),
                        ref_direction: Vector3::new(1.0, 0.0, 0.0),
                        radius,
                    },
                    source_object: None,
                });
                chamfered.model.edges.push(Edge {
                    id: edge.clone(),
                    curve: Some(curve),
                    start: VertexId("vertex".into()),
                    end: VertexId("vertex".into()),
                    param_range: None,
                    tolerance: None,
                });
                chamfered.model.coedges.push(Coedge {
                    id: coedge.clone(),
                    owner_loop: loop_id,
                    edge,
                    next: coedge.clone(),
                    previous: coedge.clone(),
                    radial_next: coedge,
                    sense: Sense::Forward,
                    pcurves: Vec::new(),
                });
            }
        }
    }
    assert_eq!(
        crate::native::simple_hole_chamfers(&chamfered, &templates, &outputs),
        std::collections::BTreeMap::from([
            (
                "hole-a".into(),
                cadmpeg_ir::features::HoleKind::Chamfer {
                    diameter: cadmpeg_ir::features::Length(7.1),
                    angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
                },
            ),
            (
                "hole-b".into(),
                cadmpeg_ir::features::HoleKind::Chamfer {
                    diameter: cadmpeg_ir::features::Length(7.1),
                    angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
                },
            ),
        ])
    );
    assert_eq!(
        crate::native::simple_hole_chamfers(
            &chamfered,
            &templates,
            &std::collections::BTreeMap::new(),
        ),
        crate::native::simple_hole_chamfers(&chamfered, &templates, &outputs)
    );
    let mut sheet = chamfered.clone();
    sheet.model.bodies[0].kind = BodyKind::Sheet;
    assert!(crate::native::simple_hole_chamfers(&sheet, &templates, &outputs).is_empty());
    let mut unrelated = chamfered.clone();
    unrelated.model.surfaces.push(Surface {
        id: SurfaceId("unrelated-cone".into()),
        geometry: SurfaceGeometry::Cone {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 1.0, 0.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 0.0,
            ratio: 0.0,
            half_angle: 0.0,
        },
        source_object: None,
    });
    unrelated.model.faces.push(Face {
        id: FaceId("unrelated-cone-face".into()),
        shell: ShellId("unrelated-shell".into()),
        surface: SurfaceId("unrelated-cone".into()),
        sense: Sense::Reversed,
        loops: vec![LoopId("unrelated-a".into()), LoopId("unrelated-b".into())],
        name: None,
        color: None,
        tolerance: None,
    });
    assert_eq!(
        crate::native::simple_hole_chamfers(&unrelated, &templates, &outputs),
        crate::native::simple_hole_chamfers(&chamfered, &templates, &outputs)
    );
    let mut unequal_chamfers = chamfered;
    let CurveGeometry::Circle { radius, .. } =
        &mut unequal_chamfers.model.curves.last_mut().unwrap().geometry
    else {
        unreachable!()
    };
    *radius += 0.1;
    assert!(
        crate::native::simple_hole_chamfers(&unequal_chamfers, &templates, &outputs).is_empty()
    );

    let mut mismatched = ir;
    let SurfaceGeometry::Cylinder { radius, .. } = &mut mismatched.model.surfaces[1].geometry
    else {
        unreachable!()
    };
    *radius = 3.0;
    assert!(
        crate::native::simple_hole_diameters(&mismatched, &templates, &[group], &outputs,)
            .is_empty()
    );
}

#[test]
fn nx_offset_feature_requires_one_output_image_and_one_exact_distance() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};
    use cadmpeg_ir::geometry::ProceduralSurface;
    use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let output = BodyId("nx:s4:body#3".into());
    let make_offset = |ordinal: u32, distance: f64| ProceduralSurface {
        id: ProceduralSurfaceId(format!("nx:s4:offset-construction#{ordinal}")),
        surface: SurfaceId(format!("nx:s4:offset-surf#{ordinal}")),
        definition: ProceduralSurfaceDefinition::Offset {
            support: SurfaceId(format!("nx:s4:nurbs-surf#{ordinal}")),
            distance,
            u_sense: Some(1),
            v_sense: Some(1),
            extension_flags: Vec::new(),
        },
        cache_fit_tolerance: None,
    };
    for ordinal in 0..2 {
        let procedural = make_offset(ordinal, 30.0);
        attach_test_body_surface(&mut ir, &output, procedural.surface.clone());
        ir.model.procedural_surfaces.push(procedural);
    }

    let (definition, supports) =
        crate::native::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("unique offset distance");
    assert_eq!(supports.len(), 2);
    assert!(matches!(
        definition,
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Native(_),
            distance: None,
        }
    ));

    let input = BodyId("nx:s4:body#input".into());
    for ordinal in 0..2 {
        attach_test_body_surface(
            &mut ir,
            &input,
            SurfaceId(format!("nx:s4:nurbs-surf#{ordinal}")),
        );
    }
    let (definition, _) =
        crate::native::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("uniquely faced supports");
    assert!(matches!(
        definition,
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Resolved { faces, .. },
            distance: Some(cadmpeg_ir::features::Length(30.0)),
        } if faces.len() == 2
    ));

    for face in ir.model.faces.iter_mut().filter(|face| {
        face.surface.0 == "nx:s4:nurbs-surf#0" || face.surface.0 == "nx:s4:nurbs-surf#1"
    }) {
        face.sense = cadmpeg_ir::topology::Sense::Reversed;
    }
    let (definition, _) =
        crate::native::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("uniformly reversed support faces");
    assert!(matches!(
        definition,
        FeatureDefinition::OffsetSurface {
            distance: Some(cadmpeg_ir::features::Length(-30.0)),
            ..
        }
    ));

    ir.model
        .faces
        .iter_mut()
        .find(|face| face.surface == SurfaceId("nx:s4:nurbs-surf#0".into()))
        .expect("first support face")
        .sense = cadmpeg_ir::topology::Sense::Forward;
    let (definition, _) =
        crate::native::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("mixed support-face orientations retain offset family");
    assert!(matches!(
        definition,
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Resolved { .. },
            distance: None,
        }
    ));

    let mut ambiguous = ir.clone();
    attach_test_body_surface(
        &mut ambiguous,
        &BodyId("nx:s4:body#duplicate".into()),
        SurfaceId("nx:s4:nurbs-surf#0".into()),
    );
    let (definition, _) =
        crate::native::offset_surface_feature_definition(&ambiguous, std::slice::from_ref(&output))
            .expect("offset semantics survive ambiguous face identity");
    assert!(matches!(
        definition,
        FeatureDefinition::OffsetSurface {
            faces: FaceSelection::Native(_),
            distance: None,
        }
    ));

    ir.model.procedural_surfaces.push(make_offset(99, -40.0));
    assert!(
        crate::native::offset_surface_feature_definition(&ir, std::slice::from_ref(&output))
            .is_some()
    );
    ir.model.procedural_surfaces.pop();

    let conflicting = make_offset(2, -30.0);
    attach_test_body_surface(&mut ir, &output, conflicting.surface.clone());
    ir.model.procedural_surfaces.push(conflicting);
    assert!(crate::native::offset_surface_feature_definition(&ir, &[output]).is_none());
}

#[test]
fn nx_circular_cone_offsets_resolve_across_equivalent_axis_origins() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    let angle = std::f64::consts::FRAC_PI_6;
    let support = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 4.0,
        ratio: 1.0,
        half_angle: angle,
    };
    let expected = 2.0;
    let axial_shift = -expected * angle.sin();
    let offset = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, axial_shift),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 4.0 + expected * angle.cos(),
        ratio: 1.0,
        half_angle: angle,
    };

    let distance = crate::decode::analytic_surface_offset(&support, &offset).expect("offset");
    assert!((distance - expected).abs() <= 1e-12);
    let reverse = crate::decode::analytic_surface_offset(&offset, &support).expect("reverse");
    assert!((reverse + expected).abs() <= 1e-12);

    let mut lateral = offset.clone();
    let SurfaceGeometry::Cone { origin, .. } = &mut lateral else {
        unreachable!()
    };
    origin.x = 0.1;
    assert!(crate::decode::analytic_surface_offset(&support, &lateral).is_none());

    let mut shifted_parameterization = offset.clone();
    let SurfaceGeometry::Cone { origin, .. } = &mut shifted_parameterization else {
        unreachable!()
    };
    origin.z += 0.1;
    assert!(crate::decode::analytic_surface_offset(&support, &shifted_parameterization).is_none());

    let mut elliptical = offset;
    let SurfaceGeometry::Cone { ratio, .. } = &mut elliptical else {
        unreachable!()
    };
    *ratio = 0.5;
    assert!(crate::decode::analytic_surface_offset(&support, &elliptical).is_none());
}

#[test]
fn nx_sphere_offset_lineage_follows_signed_radius_orientation() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    let sphere = |radius| SurfaceGeometry::Sphere {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius,
    };
    assert_eq!(
        crate::decode::analytic_surface_offset(&sphere(4.0), &sphere(6.5)),
        Some(2.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&sphere(-4.0), &sphere(-6.5)),
        Some(2.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&sphere(-6.5), &sphere(-4.0)),
        Some(-2.5)
    );
    assert!(crate::decode::analytic_surface_offset(&sphere(4.0), &sphere(-6.5)).is_none());
}

#[test]
fn nx_torus_offset_lineage_requires_one_ring_orientation() {
    use cadmpeg_ir::geometry::SurfaceGeometry;
    use cadmpeg_ir::math::{Point3, Vector3};

    let torus = |minor_radius| SurfaceGeometry::Torus {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 10.0,
        minor_radius,
    };
    assert_eq!(
        crate::decode::analytic_surface_offset(&torus(2.0), &torus(3.5)),
        Some(1.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&torus(-2.0), &torus(-3.5)),
        Some(1.5)
    );
    assert_eq!(
        crate::decode::analytic_surface_offset(&torus(-3.5), &torus(-2.0)),
        Some(-1.5)
    );
    assert!(crate::decode::analytic_surface_offset(&torus(2.0), &torus(-3.5)).is_none());
    assert!(crate::decode::analytic_surface_offset(&torus(2.0), &torus(10.0)).is_none());
}

#[test]
fn nx_thicken_feature_uses_the_magnitude_of_one_owned_offset_distance() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length, ThickenSide};
    use cadmpeg_ir::geometry::ProceduralSurface;
    use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let output = BodyId("nx:s4:body#3".into());
    let make_offset = |ordinal: u32, distance: f64| ProceduralSurface {
        id: ProceduralSurfaceId(format!("nx:s4:offset-construction#{ordinal}")),
        surface: SurfaceId(format!("nx:s4:offset-surf#{ordinal}")),
        definition: ProceduralSurfaceDefinition::Offset {
            support: SurfaceId(format!("nx:s4:nurbs-surf#{ordinal}")),
            distance,
            u_sense: Some(1),
            v_sense: Some(1),
            extension_flags: Vec::new(),
        },
        cache_fit_tolerance: None,
    };
    for ordinal in 0..2 {
        let procedural = make_offset(ordinal, -12.5);
        attach_test_body_surface(&mut ir, &output, procedural.surface.clone());
        ir.model.procedural_surfaces.push(procedural);
    }

    let (definition, supports) =
        crate::native::thicken_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("unique nonzero offset distance");
    assert_eq!(supports.len(), 2);
    assert!(matches!(
        definition,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Native(_),
            thickness: Some(Length(12.5)),
            side: None,
        }
    ));

    let mut sheet_output = ir.clone();
    sheet_output
        .model
        .bodies
        .iter_mut()
        .find(|body| body.id == output)
        .expect("output body")
        .kind = cadmpeg_ir::topology::BodyKind::Sheet;
    assert!(crate::native::thicken_feature_definition(
        &sheet_output,
        std::slice::from_ref(&output)
    )
    .is_none());

    let input = BodyId("nx:s4:body#input".into());
    for ordinal in 0..2 {
        attach_test_body_surface(
            &mut ir,
            &input,
            SurfaceId(format!("nx:s4:nurbs-surf#{ordinal}")),
        );
    }
    let (definition, _) =
        crate::native::thicken_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("uniquely faced supports");
    assert!(matches!(
        definition,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Resolved { faces, .. },
            side: Some(ThickenSide::Reverse),
            ..
        } if faces.len() == 2
    ));

    ir.model
        .faces
        .iter_mut()
        .find(|face| face.surface == SurfaceId("nx:s4:nurbs-surf#1".into()))
        .expect("second support face")
        .sense = cadmpeg_ir::topology::Sense::Reversed;
    let (definition, _) =
        crate::native::thicken_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("mixed support senses preserve thicken semantics");
    assert!(matches!(
        definition,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Resolved { .. },
            side: None,
            ..
        }
    ));

    ir.model.procedural_surfaces.push(make_offset(99, 40.0));
    assert!(
        crate::native::thicken_feature_definition(&ir, std::slice::from_ref(&output)).is_some()
    );
    ir.model.procedural_surfaces.pop();

    let conflicting = make_offset(2, 12.5);
    attach_test_body_surface(&mut ir, &output, conflicting.surface.clone());
    ir.model.procedural_surfaces.push(conflicting);
    assert!(crate::native::thicken_feature_definition(&ir, &[output]).is_none());

    let zero_output = BodyId("nx:s4:body#4".into());
    let zero = make_offset(3, 0.0);
    attach_test_body_surface(&mut ir, &zero_output, zero.surface.clone());
    ir.model.procedural_surfaces.push(zero);
    assert!(crate::native::thicken_feature_definition(&ir, &[zero_output]).is_none());
}

#[test]
fn nx_thicken_symmetric_offsets_require_identical_support_sets() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, Length, ThickenSide};
    use cadmpeg_ir::geometry::ProceduralSurface;
    use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let output = BodyId("nx:s4:body#symmetric".into());
    let input = BodyId("nx:s4:body#input".into());
    let support = SurfaceId("nx:s4:nurbs-surf#0".into());
    attach_test_body_surface(&mut ir, &input, support.clone());
    let make_offset = |ordinal: u32, support: SurfaceId, distance: f64| ProceduralSurface {
        id: ProceduralSurfaceId(format!("nx:s4:offset-construction#{ordinal}")),
        surface: SurfaceId(format!("nx:s4:offset-surf#{ordinal}")),
        definition: ProceduralSurfaceDefinition::Offset {
            support,
            distance,
            u_sense: Some(1),
            v_sense: Some(1),
            extension_flags: Vec::new(),
        },
        cache_fit_tolerance: None,
    };
    for (ordinal, distance) in [(0, -6.25), (1, 6.25)] {
        let procedural = make_offset(ordinal, support.clone(), distance);
        attach_test_body_surface(&mut ir, &output, procedural.surface.clone());
        ir.model.procedural_surfaces.push(procedural);
    }

    let (definition, supports) =
        crate::native::thicken_feature_definition(&ir, std::slice::from_ref(&output))
            .expect("matched symmetric offsets");
    assert_eq!(supports, [support.clone()]);
    assert!(matches!(
        definition,
        FeatureDefinition::Thicken {
            faces: FaceSelection::Resolved { faces, .. },
            thickness: Some(Length(12.5)),
            side: Some(ThickenSide::Both),
        } if faces.len() == 1
    ));

    let mut mismatched_support = ir.clone();
    let ProceduralSurfaceDefinition::Offset { support, .. } = &mut mismatched_support
        .model
        .procedural_surfaces
        .last_mut()
        .expect("positive offset")
        .definition
    else {
        unreachable!()
    };
    *support = SurfaceId("nx:s4:nurbs-surf#other".into());
    assert!(crate::native::thicken_feature_definition(
        &mismatched_support,
        std::slice::from_ref(&output)
    )
    .is_none());

    let ProceduralSurfaceDefinition::Offset { distance, .. } = &mut ir
        .model
        .procedural_surfaces
        .last_mut()
        .expect("positive offset")
        .definition
    else {
        unreachable!()
    };
    *distance = 7.0;
    assert!(
        crate::native::thicken_feature_definition(&ir, std::slice::from_ref(&output)).is_none()
    );
}

#[test]
fn nx_blend_feature_requires_one_output_image_and_circular_result_carriers() {
    use cadmpeg_ir::features::{FaceSelection, FeatureDefinition, RadiusForm, RadiusSpec};
    use cadmpeg_ir::geometry::{
        BlendCrossSection, BlendRadiusLaw, BlendSupport, ProceduralSurface,
        ProceduralSurfaceDefinition,
    };
    use cadmpeg_ir::ids::{BodyId, ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let output = BodyId("nx:s4:body#3".into());
    let support_a = SurfaceId("support-a".into());
    let support_b = SurfaceId("support-b".into());
    let support_c = SurfaceId("support-c".into());
    assert_eq!(
        crate::native::blend_support_bipartition(vec![
            [support_a.clone(), support_b.clone()],
            [support_b.clone(), support_c.clone()],
        ]),
        Some((
            vec![support_a.clone(), support_c.clone()],
            vec![support_b.clone()],
        ))
    );
    assert!(crate::native::blend_support_bipartition(vec![
        [support_a.clone(), support_b.clone()],
        [support_b.clone(), support_c.clone()],
        [support_c, support_a],
    ])
    .is_none());
    assert!(crate::native::blend_support_bipartition(vec![
        [SurfaceId("a".into()), SurfaceId("b".into())],
        [SurfaceId("c".into()), SurfaceId("d".into())],
    ])
    .is_none());
    let make_blend = |ordinal: u32, radius: BlendRadiusLaw| ProceduralSurface {
        id: ProceduralSurfaceId(format!("nx:s4:blend-construction#{ordinal}")),
        surface: SurfaceId(format!("nx:s4:blend-surf#{ordinal}")),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [None, None],
            spine: None,
            radius,
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
    };
    let first = make_blend(0, BlendRadiusLaw::Constant { signed_radius: 5.0 });
    attach_test_body_surface(&mut ir, &output, first.surface.clone());
    ir.model.procedural_surfaces.push(first);
    let second = make_blend(
        1,
        BlendRadiusLaw::Constant {
            signed_radius: -5.0,
        },
    );
    attach_test_body_surface(&mut ir, &output, second.surface.clone());
    ir.model.procedural_surfaces.push(second);

    let (definition, surfaces) = crate::native::blend_feature_definition(
        &ir,
        std::slice::from_ref(&output),
        crate::native::NxBlendFamily::Edge,
    )
    .expect("one circular constant-radius blend result");
    assert_eq!(surfaces.len(), 2);
    assert!(matches!(
        definition,
        FeatureDefinition::Fillet {
            radius: RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(5.0)
            },
            ..
        }
    ));
    let (definition, _) = crate::native::blend_feature_definition(
        &ir,
        std::slice::from_ref(&output),
        crate::native::NxBlendFamily::Face,
    )
    .expect("face blend retains unresolved supports");
    assert!(matches!(
        definition,
        FeatureDefinition::FaceBlend {
            first_faces: FaceSelection::Unresolved,
            second_faces: FaceSelection::Unresolved,
            radius: RadiusSpec::Constant { .. },
        }
    ));

    let mut face_blend_ir = ir.clone();
    let first_support = SurfaceId("nx:s4:blend-support#a".into());
    let second_support = SurfaceId("nx:s4:blend-support#b".into());
    for procedural in &mut face_blend_ir.model.procedural_surfaces {
        let ProceduralSurfaceDefinition::Blend { supports, .. } = &mut procedural.definition else {
            unreachable!()
        };
        *supports = [
            Some(BlendSupport {
                surface: first_support.clone(),
                reversed: false,
            }),
            Some(BlendSupport {
                surface: second_support.clone(),
                reversed: true,
            }),
        ];
    }
    attach_test_body_surface(&mut face_blend_ir, &output, first_support);
    attach_test_body_surface(&mut face_blend_ir, &output, second_support);
    let (definition, _) = crate::native::blend_feature_definition(
        &face_blend_ir,
        std::slice::from_ref(&output),
        crate::native::NxBlendFamily::Edge,
    )
    .expect("complete blend supports");
    assert!(matches!(
        definition,
        FeatureDefinition::FaceBlend {
            first_faces: FaceSelection::Resolved { ref faces, .. },
            second_faces: FaceSelection::Resolved {
                faces: ref second,
                ..
            },
            radius: RadiusSpec::Constant { .. },
        } if faces.len() == 1 && second.len() == 1 && faces != second
    ));

    ir.model.procedural_surfaces.push(make_blend(
        99,
        BlendRadiusLaw::Constant {
            signed_radius: 17.0,
        },
    ));
    let (definition, _) = crate::native::blend_feature_definition(
        &ir,
        std::slice::from_ref(&output),
        crate::native::NxBlendFamily::Edge,
    )
    .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Fillet {
            radius: RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(5.0)
            },
            ..
        }
    ));
    ir.model.procedural_surfaces.pop();

    let conflicting = make_blend(2, BlendRadiusLaw::Constant { signed_radius: 7.0 });
    attach_test_body_surface(&mut ir, &output, conflicting.surface.clone());
    ir.model.procedural_surfaces.push(conflicting);
    let (definition, _) =
        crate::native::blend_feature_definition(&ir, &[output], crate::native::NxBlendFamily::Edge)
            .unwrap();
    assert!(matches!(
        definition,
        FeatureDefinition::Fillet {
            radius: RadiusSpec::Unresolved {
                form: Some(RadiusForm::Constant)
            },
            ..
        }
    ));
    assert!(
        crate::native::blend_feature_definition(&ir, &[], crate::native::NxBlendFamily::Edge,)
            .is_none()
    );

    let conic = ProceduralSurface {
        id: ProceduralSurfaceId("nx:s4:blend-construction#3".into()),
        surface: SurfaceId("nx:s4:blend-surf#3".into()),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [None, None],
            spine: None,
            radius: BlendRadiusLaw::Constant { signed_radius: 7.0 },
            cross_section: BlendCrossSection::Conic,
            native: None,
        },
        cache_fit_tolerance: None,
    };
    attach_test_body_surface(
        &mut ir,
        &BodyId("nx:s4:body#3".into()),
        conic.surface.clone(),
    );
    ir.model.procedural_surfaces.push(conic);
    assert!(crate::native::blend_feature_definition(
        &ir,
        &[BodyId("nx:s4:body#3".into())],
        crate::native::NxBlendFamily::Edge,
    )
    .is_none());
}

#[test]
fn nx_sketch_record_joins_exact_operation_and_ordered_input_lanes() {
    use crate::native::{
        FeatureInputBlock, FeatureOperationLabel, FeatureOperationRecord, FeatureSketchReference,
    };

    let label = FeatureOperationLabel {
        id: "nx:feature-history:operation-label#0-7".to_string(),
        section_link: "nx:feature-history#0".to_string(),
        ordinal: 7,
        value: "SKETCH".to_string(),
        object_indices: [Some(45), None, Some(81), None],
        raw_object_indices: [vec![45], vec![0xff], vec![81], vec![0xff]],
        source_offset: 700,
    };
    let record = FeatureOperationRecord {
        id: "nx:feature-history:operation-record#0-7".to_string(),
        operation_label: label.id.clone(),
        ordinal: 7,
        byte_len: 173,
        sha256: "00".repeat(32),
        payload_byte_len: 140,
        payload_sha256: "11".repeat(32),
        payload_source_offset: 733,
        source_offset: 700,
    };
    let input = |slot, index| FeatureInputBlock {
        id: format!("nx:feature-history:input-block#0-7-{slot}"),
        operation_label: label.id.clone(),
        input_slot: slot,
        object_index: index,
        raw_object_index: vec![index as u8],
        data_block: format!("nx:om-data-blocks-2:block#{index}"),
        source_offset: 710 + u64::from(slot),
    };
    let inputs = [input(2, 81), input(0, 45)];
    let reference = |ordinal, index| FeatureSketchReference {
        id: format!("nx:feature-history:sketch-reference#0-7-{ordinal}"),
        operation_label: label.id.clone(),
        ordinal,
        declared_count: 2,
        terminal: ordinal == 1,
        object_index: index,
        raw_object_index: vec![0xf0, index as u8],
        data_block: Some(format!("nx:om-data-blocks-2:block#{index}")),
        source_offset: 740 + u64::from(ordinal),
    };
    let references = [reference(1, 97), reference(0, 96)];

    let sketches = crate::native::feature_sketch_records(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        &inputs,
        &references,
    );
    assert_eq!(sketches.len(), 1);
    assert_eq!(sketches[0].ordinal, 7);
    assert_eq!(
        sketches[0].operation_record,
        "nx:feature-history:operation-record#0-7"
    );
    assert_eq!(
        sketches[0].input_blocks,
        [
            "nx:feature-history:input-block#0-7-0",
            "nx:feature-history:input-block#0-7-2"
        ]
    );
    assert_eq!(
        sketches[0].payload_references,
        [
            "nx:feature-history:sketch-reference#0-7-0",
            "nx:feature-history:sketch-reference#0-7-1"
        ]
    );
    let mut duplicate_record = record.clone();
    duplicate_record.id.push_str("-duplicate");
    assert!(crate::native::feature_sketch_records(
        std::slice::from_ref(&label),
        &[record.clone(), duplicate_record],
        &inputs,
        &references,
    )
    .is_empty());
    let construction = crate::native::feature_sketch_construction_inputs(&sketches, &references);
    assert_eq!(construction.len(), 1);
    assert_eq!(
        construction[0].member_references,
        ["nx:feature-history:sketch-reference#0-7-0"]
    );
    assert_eq!(
        construction[0].member_data_blocks,
        ["nx:om-data-blocks-2:block#96"]
    );
    assert_eq!(
        construction[0].terminal_reference,
        "nx:feature-history:sketch-reference#0-7-1"
    );
    assert_eq!(
        construction[0].terminal_data_block,
        "nx:om-data-blocks-2:block#97"
    );

    let mut malformed = references;
    malformed[0].ordinal = 2;
    assert!(crate::native::feature_sketch_construction_inputs(&sketches, &malformed).is_empty());
}

#[test]
fn nx_sketch_payload_join_preserves_order_and_cross_block_values() {
    let ids = vec!["block#2".to_string(), "block#3".to_string()];
    let blocks = std::collections::BTreeMap::from([
        ("block#2".to_string(), (&[0x30, 0x43][..], 120_u64)),
        (
            "block#3".to_string(),
            (&[0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72][..], 900_u64),
        ),
    ]);
    let joined = crate::native::join_data_block_bytes(&ids, &blocks).unwrap();
    assert_eq!(joined.0, [0x30, 0x43, 0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72]);
    assert_eq!(joined.1, [0, 2]);
    assert_eq!(joined.2, [2, 6]);
    assert_eq!(joined.3, [120, 900]);

    let missing = vec!["block#2".to_string(), "missing".to_string()];
    assert!(crate::native::join_data_block_bytes(&missing, &blocks).is_none());
}

#[test]
fn nx_offset_store_block_bytes_follow_catalog_identity() {
    let control = crate::om::EntityRecord {
        object_id: None,
        object_id_offset: None,
        offset: 5,
        bytes: &[0xaa],
    };
    let first = crate::om::EntityRecord {
        object_id: None,
        object_id_offset: None,
        offset: 6,
        bytes: &[0xbb],
    };
    let second = crate::om::EntityRecord {
        object_id: None,
        object_id_offset: None,
        offset: 7,
        bytes: &[0xcc],
    };
    let controlled = crate::native::offset_data_block_bytes_for_section(
        3,
        100,
        Some(&control),
        &[first.clone(), second.clone()],
    );
    assert_eq!(
        controlled["nx:om-data-blocks-3:block#0"],
        (&[0xaa][..], 105)
    );
    assert_eq!(
        controlled["nx:om-data-blocks-3:block#1"],
        (&[0xbb][..], 106)
    );
    assert_eq!(
        controlled["nx:om-data-blocks-3:block#2"],
        (&[0xcc][..], 107)
    );

    let control_free =
        crate::native::offset_data_block_bytes_for_section(4, 200, None, &[first, second]);
    assert_eq!(
        control_free["nx:om-data-blocks-4:block#0"],
        (&[0xbb][..], 206)
    );
    assert_eq!(
        control_free["nx:om-data-blocks-4:block#1"],
        (&[0xcc][..], 207)
    );
}

#[test]
fn nx_feature_parameter_binding_joins_only_resolved_input_references() {
    use crate::native::{DataBlockReference, FeatureInputBlock};

    let input = FeatureInputBlock {
        id: "nx:feature-history:input-block#0-7-0".to_string(),
        operation_label: "nx:feature-history:operation-label#0-7".to_string(),
        input_slot: 0,
        object_index: 45,
        raw_object_index: vec![45],
        data_block: "nx:om-data-blocks-2:block#45".to_string(),
        source_offset: 700,
    };
    let reference = |ordinal: u32, declaration: Option<&str>| DataBlockReference {
        id: format!("nx:om-data-block-references-2-45:reference#{ordinal}"),
        data_block: input.data_block.clone(),
        ordinal,
        object_id: 201 + ordinal,
        raw_object_id: vec![0x80, (201 + ordinal) as u8],
        target_record: Some(format!("nx:om-record-directory-0:entry#{ordinal}")),
        target_expression_declaration: declaration.map(str::to_string),
        source_offset: 800 + u64::from(ordinal),
    };
    let references = [
        reference(0, Some("nx:om-expression-declarations-0:declaration#3")),
        reference(1, None),
    ];

    let expression = crate::native::Expression {
        id: "nx:om-entry-9:expression#3".to_string(),
        object_id: Some(201),
        record: None,
        declaration: Some("nx:om-expression-declarations-0:declaration#3".to_string()),
        name: "p3".to_string(),
        parameter_index: Some(3),
        qualifier: None,
        unit: crate::native::ExpressionUnit::Millimeter,
        expression: "12".to_string(),
        value: Some(12.0),
        source_entry: "/Root/UG_PART/UG_PART".to_string(),
        source_table: "table".to_string(),
        source_offset: 900,
    };
    let bindings = crate::native::feature_parameter_bindings(
        std::slice::from_ref(&input),
        &references,
        std::slice::from_ref(&expression),
    );
    assert_eq!(bindings.len(), 1);
    assert_eq!(
        bindings[0].id,
        "nx:feature-history:parameter-binding#0-7-0-0"
    );
    assert_eq!(bindings[0].input_slot, 0);
    assert_eq!(bindings[0].reference_ordinal, 0);
    assert_eq!(bindings[0].object_id, 201);
    assert_eq!(
        bindings[0].expression_declaration,
        "nx:om-expression-declarations-0:declaration#3"
    );
    assert_eq!(
        bindings[0].expression.as_deref(),
        Some("nx:om-entry-9:expression#3")
    );

    let mut duplicate = expression.clone();
    duplicate.id = "nx:om-entry-9:expression#30".to_string();
    let ambiguous =
        crate::native::feature_parameter_bindings(&[input], &references, &[expression, duplicate]);
    assert_eq!(ambiguous.len(), 1);
    assert_eq!(ambiguous[0].expression, None);
}

#[test]
fn segment_order_pairs_delta_across_intervening_non_history_stream() {
    use crate::parasolid::{Stream, StreamKind};
    use std::collections::BTreeSet;

    let stream = |kind, schema: Option<&str>, file_offset| Stream {
        file_offset,
        inflated: Vec::new(),
        kind,
        schema: schema.map(str::to_string),
    };
    let streams = vec![
        stream(StreamKind::Partition, Some("SCH_A"), 10),
        stream(StreamKind::Preview, None, 20),
        stream(StreamKind::Deltas, Some("SCH_A"), 30),
        stream(StreamKind::Partition, Some("SCH_B"), 40),
        stream(StreamKind::Deltas, Some("SCH_A"), 50),
        stream(StreamKind::Deltas, Some("SCH_B"), 60),
    ];
    let eligible = BTreeSet::from([2usize, 5]);
    assert_eq!(
        crate::native::pair_stream_indices(&streams, Some(&eligible)),
        std::collections::BTreeMap::from([(0, vec![2]), (3, vec![5])])
    );
}

#[test]
fn decode_links_segment_index_words_to_direct_and_separated_om_sections() {
    for (separated, expected_separator) in [(false, 0), (true, 4)] {
        let file =
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_om_payload(separated))]);
        let result = NxCodec
            .decode(&mut Cursor::new(file), &DecodeOptions::default())
            .unwrap();
        let links = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<crate::native::SegmentOmLink>("segment_om_links")
            .unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].row, "nx:segment-index:row#0");
        assert_eq!(links[0].slot, crate::native::SegmentIndexSlot::TypeCode);
        assert_eq!(
            links[0].schema_role,
            crate::native::OmSchemaRole::FeatureHistory
        );
        assert_eq!(links[0].separator_byte_len, expected_separator);
        assert_eq!(
            links[0].section_offset,
            links[0].source_offset + u64::from(expected_separator)
        );
    }
}

#[test]
fn feature_history_links_follow_unique_physical_section_order() {
    use crate::native::{OmSchemaRole, SegmentIndexSlot, SegmentOmLink};

    let link = |id: &str, schema_role, source_offset, section_offset| SegmentOmLink {
        id: id.to_string(),
        row: format!("row-{id}"),
        slot: SegmentIndexSlot::Value,
        schema_role,
        separator_byte_len: (section_offset - source_offset) as u32,
        source_offset,
        section_offset,
    };
    let links = crate::native::canonical_feature_history_links([
        link("late", OmSchemaRole::FeatureHistory, 300, 300),
        link("model", OmSchemaRole::Model, 50, 50),
        link("duplicate", OmSchemaRole::FeatureHistory, 100, 100),
        link("early", OmSchemaRole::FeatureHistory, 100, 100),
    ]);

    assert_eq!(
        links
            .iter()
            .map(|link| (link.id.as_str(), link.section_offset))
            .collect::<Vec<_>>(),
        [("duplicate", 100), ("late", 300)]
    );
}

#[test]
fn decode_orders_and_deduplicates_linked_feature_history_sections() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        multi_section_feature_history_payload(),
    )]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let namespace = result.ir.native.namespace("nx").unwrap();
    let links = namespace
        .arena_as::<crate::native::SegmentOmLink>("segment_om_links")
        .unwrap();
    assert_eq!(links.len(), 4);
    let labels = namespace
        .arena_as::<crate::native::FeatureOperationLabel>("feature_operation_labels")
        .unwrap();
    assert_eq!(
        labels
            .iter()
            .map(|label| (label.value.as_str(), label.ordinal))
            .collect::<Vec<_>>(),
        [("BLOCK", 0), ("UNITE", 0)]
    );
    assert_ne!(labels[0].section_link, labels[1].section_link);
    assert_eq!(
        labels[0].raw_object_indices,
        [
            vec![0x01],
            vec![0x82, 0x40],
            vec![0x90, 0x17, 0xd3],
            vec![0xff]
        ]
    );
    assert_eq!(labels[1].raw_object_indices, labels[0].raw_object_indices);
    let records = namespace
        .arena_as::<crate::native::FeatureOperationRecord>("feature_operation_records")
        .unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].operation_label, labels[0].id);
    assert_eq!(records[1].operation_label, labels[1].id);
    assert_eq!(
        result
            .ir
            .model
            .features
            .iter()
            .map(|feature| feature.name.as_deref())
            .collect::<Vec<_>>(),
        [Some("BLOCK"), Some("UNITE")]
    );
}

#[test]
fn decoded_feature_ids_preserve_double_digit_operation_order() {
    let section = size_framed_om_section_with_repeated_operations(12);
    let mut payload = Vec::new();
    for word in [24_u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.extend_from_slice(&section);
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let labels = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::FeatureOperationLabel>("feature_operation_labels")
        .unwrap();

    assert_eq!(
        labels.iter().map(|label| label.ordinal).collect::<Vec<_>>(),
        (0..12).collect::<Vec<_>>()
    );
    assert!(labels
        .windows(2)
        .all(|pair| pair[0].id.as_str() < pair[1].id.as_str()));
}

#[test]
fn decode_retains_role_scoped_om_record_area_header() {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_om_record_area_payload())]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let areas = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::OmRecordArea>("om_record_areas")
        .unwrap();
    assert_eq!(areas.len(), 1);
    assert_eq!(
        areas[0].schema_role,
        crate::native::OmSchemaRole::FeatureHistory
    );
    assert_eq!(areas[0].control_words, [13, 14, 44]);
    assert_eq!(areas[0].product_version, "NX 2027.3102");
    assert!(areas[0].byte_len > 12);
    assert_eq!(areas[0].sha256.len(), 64);
    let labels = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::FeatureOperationLabel>("feature_operation_labels")
        .unwrap();
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].ordinal, 0);
    assert_eq!(labels[0].value, "UNITE");
    assert_eq!(
        labels[0].object_indices,
        [Some(1), Some(576), Some(6099), None]
    );
    assert_eq!(labels[0].section_link, areas[0].section_link);
    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::FeatureOperationRecord>("feature_operation_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].operation_label, labels[0].id);
    assert!(records[0].byte_len > 40);
    assert_eq!(records[0].sha256.len(), 64);
    let booleans = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::FeatureBooleanOperation>("feature_boolean_operations")
        .unwrap();
    assert_eq!(booleans.len(), 1);
    assert_eq!(booleans[0].kind, crate::native::FeatureBooleanKind::Unite);
    assert_eq!(booleans[0].target_object_index, 6466);
    assert_eq!(booleans[0].tool_object_indices, [6476, 127]);
    let body_references = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::FeatureBodyReference>("feature_body_references")
        .unwrap();
    assert_eq!(body_references.len(), 1);
    assert_eq!(body_references[0].operation_label, labels[0].id);
    assert_eq!(body_references[0].body_object_index, 6466);
    let body_reference_occurrences = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::FeatureBodyReferenceOccurrence>(
            "feature_body_reference_occurrences",
        )
        .unwrap();
    assert_eq!(body_reference_occurrences.len(), 1);
    assert_eq!(body_reference_occurrences[0].operation_label, labels[0].id);
    assert_eq!(body_reference_occurrences[0].ordinal, 0);
    assert_eq!(body_reference_occurrences[0].body_object_index, 6466);
    let feature = result.ir.model.features.first().expect("neutral feature");
    assert_eq!(feature.name.as_deref(), Some("UNITE"));
    assert_eq!(feature.suppressed, None);
    assert_eq!(feature.native_ref.as_deref(), Some(labels[0].id.as_str()));
    assert_eq!(
        feature.source_properties.get("body_reference.0"),
        Some(&"6466".to_string())
    );
    assert!(matches!(
        &feature.definition,
        cadmpeg_ir::features::FeatureDefinition::Combine {
            target: cadmpeg_ir::features::BodySelection::Native(target),
            tools: cadmpeg_ir::features::BodySelection::Native(tools),
            op: cadmpeg_ir::features::BooleanOp::Join,
        } if target == "nx:om-object-index#6466" && tools == "nx:om-object-indices#6476,127"
    ));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_feature_header_input_to_unique_data_block() {
    let file = prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        segment_om_record_area_with_input_store_payload(),
    )]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let inputs = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::FeatureInputBlock>("feature_input_blocks")
        .unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].input_slot, 0);
    assert_eq!(inputs[0].object_index, 1);
    assert!(inputs[0].data_block.ends_with(":block#1"));
    assert_eq!(
        result.ir.model.features[0].source_properties["input_block.0"],
        inputs[0].data_block
    );
    let references = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::DataBlockReference>("data_block_references")
        .unwrap();
    assert_eq!(references.len(), 1);
    assert!(references[0].data_block.ends_with(":block#2"));
    assert_ne!(references[0].data_block, inputs[0].data_block);
    assert_eq!(references[0].object_id, 42);
    assert_eq!(references[0].target_record, None);
}

#[test]
fn feature_input_identity_groups_require_distinct_operations_and_preserve_order() {
    use crate::native::{feature_input_block_identity_groups, FeatureInputBlock};

    let input = |id: &str, operation: &str, slot: u8, block: &str, offset: u64| FeatureInputBlock {
        id: id.to_string(),
        operation_label: operation.to_string(),
        input_slot: slot,
        object_index: 7,
        raw_object_index: vec![7],
        data_block: block.to_string(),
        source_offset: offset,
    };
    let groups = feature_input_block_identity_groups(&[
        input("late", "operation-b", 1, "block-7", 30),
        input("single-a", "operation-a", 0, "block-8", 10),
        input("early", "operation-a", 2, "block-7", 20),
        input("single-b", "operation-a", 3, "block-8", 40),
    ]);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].data_block, "block-7");
    assert_eq!(groups[0].input_blocks, ["early", "late"]);
    assert_eq!(groups[0].operation_labels, ["operation-a", "operation-b"]);
    assert_eq!(groups[0].input_slots, [2, 1]);
    assert_eq!(groups[0].source_offsets, [20, 30]);
}

#[test]
fn feature_input_column_row_uses_preserve_index_row_slots() {
    use crate::native::{
        feature_input_column_row_uses, ColumnIndexRowKind, DataBlockIndexRow, FeatureInputBlock,
    };

    let input = FeatureInputBlock {
        id: "input#0000000001".into(),
        operation_label: "operation#1".into(),
        input_slot: 2,
        object_index: 7,
        raw_object_index: vec![7],
        data_block: "block#4".into(),
        source_offset: 10,
    };
    let row = DataBlockIndexRow {
        id: "row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        first_index: 20,
        raw_first_index: vec![20],
        flag: 3,
        indices: [4, 4, 5, 6],
        raw_indices: [vec![4], vec![4], vec![5], vec![6]],
        data_blocks: [
            "block#4".into(),
            "block#4".into(),
            "block#5".into(),
            "block#6".into(),
        ],
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
        source_offset: 100,
        first_index_source_offset: 103,
        index_source_offsets: [108, 109, 110, 111],
    };

    let uses = feature_input_column_row_uses(&[input], &[row], &[], &[], &[]);
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].input_block, "input#0000000001");
    assert_eq!(uses[0].operation_label, "operation#1");
    assert_eq!(uses[0].input_slot, 2);
    assert_eq!(uses[0].row_kind, ColumnIndexRowKind::Index);
    assert_eq!(uses[0].column_row, "row#3");
    assert_eq!(uses[0].row_slot, 0);
    assert_eq!(uses[0].source_offset, 108);
    assert_eq!(uses[1].row_slot, 1);
    assert_eq!(uses[1].source_offset, 109);
}

#[test]
fn feature_input_column_row_uses_preserve_linked_row_slots() {
    use crate::native::{
        feature_input_column_row_uses, feature_input_column_targets, ColumnIndexRowKind,
        DataBlockColumnIndexTable, DataBlockLinkedIndexRow, FeatureInputBlock,
    };

    let input = FeatureInputBlock {
        id: "input#0000000001".into(),
        operation_label: "operation#1".into(),
        input_slot: 2,
        object_index: 4,
        raw_object_index: vec![4],
        data_block: "block#4".into(),
        source_offset: 10,
    };
    let row = DataBlockLinkedIndexRow {
        id: "linked-row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        first_index: 20,
        raw_first_index: vec![20],
        discriminator: 0x16,
        target_index: 4,
        raw_target_index: vec![4],
        indices: [5, 6, 4],
        raw_indices: [vec![5], vec![6], vec![4]],
        data_blocks: [
            "block#4".into(),
            "block#5".into(),
            "block#6".into(),
            "block#4".into(),
        ],
        flag: 3,
        mode: 4,
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
        source_offset: 100,
        first_index_source_offset: 102,
        target_index_source_offset: 107,
        index_source_offsets: [112, 113, 114],
    };

    let table = DataBlockColumnIndexTable {
        id: "column-table".into(),
        section_ordinal: 0,
        opening_linked_row: row.id.clone(),
        target_rows: vec!["target-row".into()],
        linked_rows: vec!["suffix-row".into()],
        first_target_index: 4,
        last_target_index: 2,
        source_entry: "entry".into(),
        source_offset: 100,
    };
    let uses = feature_input_column_row_uses(&[input.clone()], &[], &[row.clone()], &[], &[table]);
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].input_block, "input#0000000001");
    assert_eq!(uses[0].operation_label, "operation#1");
    assert_eq!(uses[0].input_slot, 2);
    assert_eq!(uses[0].row_kind, ColumnIndexRowKind::LinkedIndex);
    assert_eq!(uses[0].column_row, "linked-row#3");
    assert_eq!(uses[0].row_slot, 0);
    assert_eq!(uses[0].source_offset, 107);
    assert_eq!(uses[1].row_slot, 3);
    assert_eq!(uses[1].source_offset, 114);
    let targets = feature_input_column_targets(&[input], &uses, &[row], &[]);
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].leading_index, Some(20));
    assert_eq!(targets[0].leading_index_source_offset, Some(102));
    assert_eq!(targets[0].discriminator, Some(0x16));
    assert_eq!(targets[0].field_indices, [5, 6, 4]);
    assert_eq!(targets[0].flag, Some(3));
    assert_eq!(targets[0].mode, 4);
}

#[test]
fn feature_input_column_row_uses_preserve_target_row_slots() {
    use crate::native::{
        feature_input_column_row_uses, feature_input_column_targets, ColumnIndexRowKind,
        DataBlockColumnIndexTable, DataBlockTargetIndexRow, FeatureInputBlock,
    };

    let input = FeatureInputBlock {
        id: "input#0000000001".into(),
        operation_label: "operation#1".into(),
        input_slot: 2,
        object_index: 4,
        raw_object_index: vec![4],
        data_block: "block#4".into(),
        source_offset: 10,
    };
    let row = DataBlockTargetIndexRow {
        id: "target-row#3".into(),
        section_ordinal: 0,
        ordinal: 3,
        target_index: 4,
        raw_target_index: vec![4],
        indices: [5, 6, 4],
        raw_indices: [vec![5], vec![6], vec![4]],
        data_blocks: [
            "block#4".into(),
            "block#5".into(),
            "block#6".into(),
            "block#4".into(),
        ],
        mode: 7,
        source_entry: "entry".into(),
        opening_data_block: "opening-block".into(),
        opening_block_offset: 8,
        source_offset: 100,
        target_index_source_offset: 105,
        index_source_offsets: [110, 111, 112],
    };

    let table = DataBlockColumnIndexTable {
        id: "column-table".into(),
        section_ordinal: 0,
        opening_linked_row: "opening-row".into(),
        target_rows: vec!["target-row#3".into()],
        linked_rows: vec!["suffix-row".into()],
        first_target_index: 5,
        last_target_index: 3,
        source_entry: "entry".into(),
        source_offset: 50,
    };
    let ambiguous = feature_input_column_row_uses(
        &[input.clone()],
        &[],
        &[],
        &[row.clone()],
        &[table.clone(), table.clone()],
    );
    assert!(ambiguous.iter().all(|use_| use_.column_table.is_none()));
    let uses = feature_input_column_row_uses(&[input.clone()], &[], &[], &[row.clone()], &[table]);
    assert_eq!(uses.len(), 2);
    assert_eq!(uses[0].input_block, "input#0000000001");
    assert_eq!(uses[0].operation_label, "operation#1");
    assert_eq!(uses[0].input_slot, 2);
    assert_eq!(uses[0].row_kind, ColumnIndexRowKind::TargetIndex);
    assert_eq!(uses[0].column_row, "target-row#3");
    assert_eq!(uses[0].column_table.as_deref(), Some("column-table"));
    assert_eq!(uses[0].row_slot, 0);
    assert_eq!(uses[0].source_offset, 105);
    assert_eq!(uses[1].row_slot, 3);
    assert_eq!(uses[1].source_offset, 112);
    let targets = feature_input_column_targets(&[input.clone()], &uses, &[], &[row.clone()]);
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].input_block, input.id);
    assert_eq!(targets[0].column_row, "target-row#3");
    assert_eq!(targets[0].column_table, "column-table");
    assert_eq!(targets[0].field_indices, [5, 6, 4]);
    assert_eq!(
        targets[0].field_data_blocks,
        ["block#5", "block#6", "block#4"]
    );
    assert_eq!(targets[0].field_source_offsets, [110, 111, 112]);
    assert_eq!(targets[0].mode, 7);
    assert_eq!(targets[0].leading_index, None);
    let mut duplicate = uses.clone();
    duplicate.push(uses[0].clone());
    assert!(feature_input_column_targets(&[input], &duplicate, &[], &[row]).is_empty());
}

#[test]
fn data_block_column_index_tables_require_complete_mode_and_target_sequence() {
    use crate::native::{
        data_block_column_index_tables, DataBlockLinkedIndexRow, DataBlockTargetIndexRow,
    };

    let linked = |id: &str, target: u32, mode: u8, offset: u64| DataBlockLinkedIndexRow {
        id: id.into(),
        section_ordinal: 2,
        ordinal: 0,
        first_index: 20,
        raw_first_index: vec![20],
        discriminator: 0x16,
        target_index: target,
        raw_target_index: vec![target as u8],
        indices: [5, 6, 7],
        raw_indices: [vec![5], vec![6], vec![7]],
        data_blocks: [
            format!("block#{target}"),
            "block#5".into(),
            "block#6".into(),
            "block#7".into(),
        ],
        flag: 3,
        mode,
        source_entry: "entry".into(),
        opening_data_block: format!("opening-block-{id}"),
        opening_block_offset: 8,
        source_offset: offset,
        first_index_source_offset: offset + 2,
        target_index_source_offset: offset + 7,
        index_source_offsets: [offset + 12, offset + 13, offset + 14],
    };
    let target = |id: &str, index: u32, mode: u8, offset: u64| DataBlockTargetIndexRow {
        id: id.into(),
        section_ordinal: 2,
        ordinal: 0,
        target_index: index,
        raw_target_index: vec![index as u8],
        indices: [5, 6, 7],
        raw_indices: [vec![5], vec![6], vec![7]],
        data_blocks: [
            format!("block#{index}"),
            "block#5".into(),
            "block#6".into(),
            "block#7".into(),
        ],
        mode,
        source_entry: "entry".into(),
        opening_data_block: format!("opening-block-{id}"),
        opening_block_offset: 8,
        source_offset: offset,
        target_index_source_offset: offset + 5,
        index_source_offsets: [offset + 10, offset + 11, offset + 12],
    };
    let linked_rows = [
        linked("opening", 63, 7, 100),
        linked("linked-59", 59, 4, 200),
        linked("linked-58", 58, 4, 225),
    ];
    let target_rows = [
        target("target-62", 62, 7, 125),
        target("target-61", 61, 7, 150),
        target("target-60", 60, 4, 175),
    ];

    let tables = data_block_column_index_tables(&linked_rows, &target_rows);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].id, "nx:om-data-block-column-index-tables:table#2");
    assert_eq!(tables[0].opening_linked_row, "opening");
    assert_eq!(
        tables[0].target_rows,
        ["target-62", "target-61", "target-60"]
    );
    assert_eq!(tables[0].linked_rows, ["linked-59", "linked-58"]);
    assert_eq!(tables[0].first_target_index, 63);
    assert_eq!(tables[0].last_target_index, 58);
    assert_eq!(tables[0].source_offset, 100);

    let mut gap = target_rows.clone();
    gap[1].target_index = 60;
    assert!(data_block_column_index_tables(&linked_rows, &gap).is_empty());
    let mut incomplete_mode = target_rows.clone();
    incomplete_mode[2].mode = 7;
    assert!(data_block_column_index_tables(&linked_rows, &incomplete_mode).is_empty());
}

#[test]
fn om_compact_index_lane_decodes_direct_extended_and_null_entries() {
    use crate::om::CompactIndex::{Null, Value};

    assert_eq!(
        crate::om::compact_indices(&[0x00, 0x7f, 0x80, 0x80, 0x81, 0x00, 0xfe, 0xff, 0xff]),
        Some(vec![
            Value(0),
            Value(127),
            Value(128),
            Value(256),
            Value(32_511),
            Null,
        ])
    );
    assert_eq!(crate::om::compact_indices(&[0x80]), None);
}

#[test]
fn om_data_block_object_frame_requires_complete_discriminator() {
    let discriminator = [
        0x00, 0x72, 0x01, 0xc0, 0x20, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x01,
        0x02, 0x80, 0xa4,
    ];
    let mut bytes = vec![0xaa, 0x81, 0x72];
    bytes.extend_from_slice(&discriminator);
    bytes.push(0xff);

    let references = crate::om::data_block_object_frames(&bytes);
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].object_id, 370);
    assert_eq!(references[0].raw_object_id, [0x81, 0x72]);
    assert_eq!(references[0].offset, 1);

    bytes.extend_from_slice(&[0x73]);
    bytes.extend_from_slice(&discriminator);
    let references = crate::om::data_block_object_frames(&bytes);
    assert_eq!(references.len(), 2);
    assert_eq!(references[1].object_id, 0x73);
    assert_eq!(references[1].raw_object_id, [0x73]);
    assert_eq!(references[1].offset, 22);

    bytes[8] ^= 1;
    let references = crate::om::data_block_object_frames(&bytes);
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].object_id, 0x73);
    let mut null = vec![0xff];
    null.extend_from_slice(&discriminator);
    assert!(crate::om::data_block_object_frames(&null).is_empty());
}

#[test]
fn om_offset_store_counted_index_lane_requires_complete_non_null_members() {
    let bytes = [
        0xaa, 0x01, 0x06, 0x42, 0x62, 0x80, 0x48, 0x80, 0x50, 0x7c, 0x01, 0x11, 0xbb,
    ];
    let lanes = crate::om::offset_store_counted_index_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 1);
    assert_eq!(lanes[0].declared_count, 6);
    assert_eq!(lanes[0].anchor, 0x42);
    assert_eq!(lanes[0].raw_anchor, [0x42]);
    assert_eq!(lanes[0].anchor_offset, 3);
    assert_eq!(
        lanes[0].members,
        vec![(0x62, 4), (0x48, 5), (0x50, 7), (0x7c, 9)]
    );
    assert_eq!(
        lanes[0].raw_members,
        [vec![0x62], vec![0x80, 0x48], vec![0x80, 0x50], vec![0x7c]]
    );

    assert!(
        crate::om::offset_store_counted_index_lanes(&[0x01, 0x03, 0x42, 0xff, 0x01, 0x11,])
            .is_empty()
    );
    assert!(
        crate::om::offset_store_counted_index_lanes(&[0x01, 0x03, 0x42, 0x80, 0x01, 0x11,])
            .is_empty()
    );
    assert!(
        crate::om::offset_store_counted_index_lanes(&[0x01, 0x03, 0x42, 0x62, 0x01, 0x10,])
            .is_empty()
    );
}

#[test]
fn om_offset_store_abr_lane_requires_sixteen_slots_and_exact_terminator() {
    let mut bytes = vec![0xaa, 0x11];
    bytes.extend_from_slice(&[0xff; 6]);
    bytes.extend_from_slice(&[0x82, 0x83]);
    bytes.extend_from_slice(&[0xff; 9]);
    bytes.extend_from_slice(&[0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03, 0xbb]);

    let lanes = crate::om::offset_store_abr_reference_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 1);
    assert_eq!(lanes[0].slots.len(), 16);
    assert_eq!(lanes[0].slots[6], (Some(643), 8));
    assert_eq!(lanes[0].raw_slots[6], [0x82, 0x83]);
    assert!(lanes[0]
        .raw_slots
        .iter()
        .enumerate()
        .all(|(slot, raw)| slot == 6 || raw == &[0xff]));
    assert!(lanes[0]
        .slots
        .iter()
        .enumerate()
        .all(|(slot, (value, _))| slot == 6 || value.is_none()));

    bytes[23] = b'X';
    assert!(crate::om::offset_store_abr_reference_lanes(&bytes).is_empty());
    bytes[23] = b'R';
    bytes.remove(18);
    assert!(crate::om::offset_store_abr_reference_lanes(&bytes).is_empty());
}

#[test]
fn om_sketch_scalar_field_requires_exact_frame_and_finite_shifted_value() {
    let bytes = [
        0xaa, 0x50, 0x59, 0x66, 0x64, 0x00, 0x30, 0x43, 0x0c, 0xcc, 0xcc, 0xcc, 0xcd, 0x72, 0xbb,
    ];
    let fields = crate::om::construction_payload_scalar_fields(&bytes);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].offset, 1);
    assert_eq!(fields[0].field_code, 0x64);
    assert!((fields[0].value - 38.1).abs() < 2.0e-12);

    let mut malformed = bytes;
    malformed[5] = 1;
    assert!(crate::om::construction_payload_scalar_fields(&malformed).is_empty());
    malformed = bytes;
    malformed[6] = 0x70;
    assert!(crate::om::construction_payload_scalar_fields(&malformed).is_empty());
}

#[test]
fn om_sketch_name_field_decodes_direct_and_extended_compact_type_codes() {
    let bytes = [
        0x66, 0x32, 0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00, 0xaa, 0x66, 0x80, 0x83,
        0x03, 0x07, b'L', b'i', b'n', b'e', b'2', 0x00,
    ];
    let fields = crate::om::construction_payload_named_fields(&bytes);
    assert_eq!(fields.len(), 2);
    assert_eq!(
        (fields[0].offset, fields[0].type_code, fields[0].value),
        (0, Some(0x32), "Point1")
    );
    assert_eq!(fields[0].raw_type_code, Some(vec![0x32]));
    assert_eq!(fields[0].type_code_offset, Some(1));
    assert_eq!(
        (fields[1].offset, fields[1].type_code, fields[1].value),
        (12, Some(0x83), "Line2")
    );
    assert_eq!(fields[1].raw_type_code, Some(vec![0x80, 0x83]));
    assert_eq!(fields[1].type_code_offset, Some(13));

    assert!(crate::om::construction_payload_named_fields(&[
        0x66, 0xff, 0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00,
    ])
    .is_empty());
    assert!(crate::om::construction_payload_named_fields(&[
        0x66, 0x32, 0x03, 0x08, b'P', b'o', b'i', b'n', b't',
    ])
    .is_empty());
}

#[test]
fn om_sketch_name_field_decodes_type_free_payload_leading_form() {
    let fields = crate::om::construction_payload_named_fields(&[
        0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1', 0x00, 0x04,
    ]);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].offset, 0);
    assert_eq!(fields[0].type_code, None);
    assert_eq!(fields[0].raw_type_code, None);
    assert_eq!(fields[0].type_code_offset, None);
    assert!(fields[0].payload_leading);
    assert_eq!(fields[0].value, "Point1");

    assert!(crate::om::construction_payload_named_fields(&[
        0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'1',
    ])
    .is_empty());
}

#[test]
fn om_offset_store_named_point_uses_minimal_consecutive_block_span() {
    let first = [
        0x03, 0x08, b'P', b'o', b'i', b'n', b't', b'7', 0x00, 0x50, 0x59, 0x66, 0x58, 0x00, 0x30,
        0x4c, 0x93, 0x33, 0x33, 0x33, 0x33, 0x07,
    ];
    let second = [
        0x45, 0x04, 0x00, 0x50, 0x59, 0x66, 0x58, 0x00, 0x30, 0x4c, 0x93, 0x33, 0x33, 0x33, 0x33,
        0x07,
    ];
    let point = crate::om::offset_store_named_point(&[&first, &second]).unwrap();
    assert_eq!(point.name, "Point7");
    assert!(point
        .values
        .iter()
        .all(|value| (*value - 57.15).abs() < 1.0e-12));
    let expected_raw: [[u8; 8]; 2] = [
        first[14..22].try_into().unwrap(),
        second[8..16].try_into().unwrap(),
    ];
    assert_eq!(point.raw_values, expected_raw);
    assert_eq!(point.value_offsets, [9, first.len() + 3]);
    assert_eq!(point.block_count, 2);

    let mut same_block = first.to_vec();
    same_block.extend_from_slice(&second);
    assert_eq!(
        crate::om::offset_store_named_point(&[&same_block])
            .unwrap()
            .block_count,
        1
    );
    assert_eq!(
        crate::om::offset_store_named_point(&[&first[..9], &first[9..], &second])
            .unwrap()
            .block_count,
        3
    );
    let mut zero = first;
    zero[7] = b'0';
    assert!(crate::om::offset_store_named_point(&[&zero, &second]).is_none());
}

#[test]
fn sketch_fixed_pair_parser_reads_signed_q1_55_atoms() {
    let bytes = [
        0x04, 0xe0, 0x48, 0x0e, 0x02, 0x03, 0x80, 0x84, 0x30, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x30, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let pairs = crate::om::sketch_payload_fixed_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].values, [0.5, -0.5]);
    assert_eq!(pairs[0].value_offsets, [8, 17]);
    assert_eq!(pairs[0].raw_values[0], [0x40, 0, 0, 0, 0, 0, 0]);

    let mut malformed = bytes;
    malformed[16] = 1;
    assert!(crate::om::sketch_payload_fixed_pairs(&malformed).is_empty());
}

#[test]
fn datum_csys_fixed_pair_requires_its_exact_branch_discriminator() {
    let mut bytes = vec![
        0x0b, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
        0x30,
    ];
    bytes.extend_from_slice(&[0x40, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0x00, 0x30]);
    bytes.extend_from_slice(&[0xc0, 0, 0, 0, 0, 0, 0]);
    let pairs = crate::om::datum_csys_payload_fixed_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].values, [0.5, -0.5]);
    assert_eq!(pairs[0].value_offsets, [15, 24]);
    assert_eq!(pairs[0].raw_values[0], [0x40, 0, 0, 0, 0, 0, 0]);

    bytes[0] = 0x08;
    assert!(crate::om::datum_csys_payload_fixed_pairs(&bytes).is_empty());
}

#[test]
fn sketch_named_records_own_fixed_pairs_within_their_intervals() {
    use crate::native::{
        feature_sketch_fixed_points, feature_sketch_payload_named_records,
        FeatureSketchConstructionPayload, FeatureSketchPayloadFixedPair, FeatureSketchPayloadName,
    };
    let payload = FeatureSketchConstructionPayload {
        id: "payload".to_string(),
        operation_label: "sketch".to_string(),
        construction_inputs: "inputs".to_string(),
        data_blocks: vec!["block".to_string()],
        byte_len: 100,
        sha256: "00".repeat(32),
        block_payload_offsets: vec![0],
        block_byte_lengths: vec![100],
        block_source_offsets: vec![1000],
    };
    let name = |id: &str, ordinal, offset| FeatureSketchPayloadName {
        id: id.to_string(),
        operation_label: "sketch".to_string(),
        construction_payload: "payload".to_string(),
        ordinal,
        type_code: Some(1),
        raw_type_code: Some(vec![1]),
        type_code_payload_offset: Some(offset + 1),
        type_code_source_offset: Some(1001 + offset),
        payload_leading: false,
        value: format!("Point{}", ordinal + 1),
        payload_offset: offset,
        source_offset: 1000 + offset,
    };
    let pair = FeatureSketchPayloadFixedPair {
        id: "pair".to_string(),
        operation_label: "sketch".to_string(),
        construction_payload: "payload".to_string(),
        ordinal: 0,
        values: [0.5, -0.5],
        raw_values: [[0; 7]; 2],
        payload_offset: 20,
        value_payload_offsets: [28, 37],
        source_offset: 1020,
        value_source_offsets: [1028, 1037],
    };

    let names = [name("first", 0, 10), name("second", 1, 50)];
    let pairs = [pair];
    let records = feature_sketch_payload_named_records(&[payload], &names, &[], &pairs);
    assert_eq!(records[0].fixed_pairs, ["pair"]);
    assert!(records[1].fixed_pairs.is_empty());
    let points = feature_sketch_fixed_points(&records, &names, &pairs);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].name, "Point1");
    assert_eq!(points[0].values, [0.5, -0.5]);
}

#[test]
fn sketch_named_point_block_uses_require_exact_shared_block_identity() {
    use crate::native::{
        feature_sketch_named_point_block_uses, FeatureSketchReference, OffsetStoreNamedPoint,
    };

    let point = OffsetStoreNamedPoint {
        id: "nx:offset-store:named-point#2-10".to_string(),
        name: "Point1".to_string(),
        data_blocks: vec!["block-10".to_string(), "block-11".to_string()],
        values: [1.0, 2.0],
        raw_values: [shifted_f64_bytes(1.0), shifted_f64_bytes(2.0)],
        value_source_offsets: [100, 120],
        source_offset: 90,
    };
    let reference = |id: &str, ordinal: u32, block: Option<&str>| FeatureSketchReference {
        id: id.to_string(),
        operation_label: "nx:feature-history:operation-label#1-4".to_string(),
        ordinal,
        declared_count: 2,
        terminal: ordinal == 1,
        object_index: 10 + ordinal,
        raw_object_index: vec![0xf0, (10 + ordinal) as u8],
        data_block: block.map(str::to_string),
        source_offset: 200 + u64::from(ordinal),
    };
    let uses = feature_sketch_named_point_block_uses(
        &[
            reference("miss", 0, Some("block-9")),
            reference("hit", 1, Some("block-11")),
            reference("unresolved", 2, None),
        ],
        &[point],
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].sketch_reference, "hit");
    assert_eq!(uses[0].reference_ordinal, 1);
    assert_eq!(uses[0].point_block_ordinal, 1);
    assert_eq!(uses[0].data_block, "block-11");
}

#[test]
fn sketch_preceding_named_point_uses_require_a_complete_unique_consecutive_lane() {
    use crate::native::{
        feature_sketch_preceding_named_point_uses, FeatureSketchReference, OffsetStoreNamedPoint,
    };

    let reference = |ordinal, terminal, block: Option<&str>| FeatureSketchReference {
        id: format!("reference-{ordinal}"),
        operation_label: "nx:feature-history:operation-label#1-4".to_string(),
        ordinal,
        declared_count: 2,
        terminal,
        object_index: 12 + ordinal,
        raw_object_index: vec![0xf0, (12 + ordinal) as u8],
        data_block: block.map(str::to_string),
        source_offset: 300 + u64::from(ordinal),
    };
    let references = [
        reference(0, false, Some("nx:om-data-blocks-2:block#12")),
        reference(1, true, Some("nx:om-data-blocks-2:block#13")),
    ];
    let point = |id: &str, blocks: &[&str]| OffsetStoreNamedPoint {
        id: id.to_string(),
        name: "Point1".to_string(),
        data_blocks: blocks.iter().map(|block| (*block).to_string()).collect(),
        values: [1.0, 2.0],
        raw_values: [shifted_f64_bytes(1.0), shifted_f64_bytes(2.0)],
        value_source_offsets: [200, 220],
        source_offset: 190,
    };
    let preceding = point(
        "nx:offset-store:named-point#2-10",
        &[
            "nx:om-data-blocks-2:block#10",
            "nx:om-data-blocks-2:block#11",
        ],
    );
    let uses =
        feature_sketch_preceding_named_point_uses(&references, std::slice::from_ref(&preceding));
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].first_sketch_reference, references[0].id);
    assert_eq!(uses[0].named_point, preceding.id);
    assert_eq!(uses[0].following_data_block, "nx:om-data-blocks-2:block#12");

    let ambiguous = point(
        "nx:offset-store:named-point#2-11",
        &["nx:om-data-blocks-2:block#11"],
    );
    assert!(feature_sketch_preceding_named_point_uses(
        &references,
        &[preceding.clone(), ambiguous]
    )
    .is_empty());
    let gap = point(
        "nx:offset-store:named-point#2-9",
        &["nx:om-data-blocks-2:block#9"],
    );
    let other_store = point(
        "nx:offset-store:named-point#3-11",
        &["nx:om-data-blocks-3:block#11"],
    );
    assert!(feature_sketch_preceding_named_point_uses(&references, &[gap, other_store]).is_empty());

    let unresolved = [references[0].clone(), reference(1, true, None)];
    assert!(feature_sketch_preceding_named_point_uses(
        &unresolved,
        std::slice::from_ref(&preceding)
    )
    .is_empty());
    let noncontiguous = [
        references[0].clone(),
        reference(2, true, Some("nx:om-data-blocks-2:block#13")),
    ];
    assert!(feature_sketch_preceding_named_point_uses(
        &noncontiguous,
        std::slice::from_ref(&preceding),
    )
    .is_empty());
    let bad_terminal = [
        references[0].clone(),
        reference(1, false, Some("nx:om-data-blocks-2:block#13")),
    ];
    assert!(feature_sketch_preceding_named_point_uses(&bad_terminal, &[preceding]).is_empty());
}

#[test]
fn sketch_point_uses_retain_identical_witnesses_and_reject_conflicts() {
    use crate::native::{
        feature_sketch_point_groups, feature_sketch_point_uses, FeatureSketchNamedPointBlockUse,
        FeatureSketchPoint, OffsetStoreNamedPoint,
    };

    let operation_label = "nx:feature-history:operation-label#1-4".to_string();
    let point = FeatureSketchPoint {
        id: "payload-point".to_string(),
        operation_label: operation_label.clone(),
        named_record: "named-record".to_string(),
        name: "Point1".to_string(),
        coordinates: [1.0, 2.0],
        scalar_fields: ["scalar-1".to_string(), "scalar-2".to_string()],
    };
    let named_point = OffsetStoreNamedPoint {
        id: "named-point".to_string(),
        name: "Point1".to_string(),
        data_blocks: vec!["block-10".to_string()],
        values: [1.0, 2.0],
        raw_values: [shifted_f64_bytes(1.0), shifted_f64_bytes(2.0)],
        value_source_offsets: [200, 220],
        source_offset: 190,
    };
    let block_use = FeatureSketchNamedPointBlockUse {
        id: "nx:feature-history:sketch-named-point-block-use#1-4-0".to_string(),
        operation_label,
        sketch_reference: "reference".to_string(),
        reference_ordinal: 0,
        named_point: named_point.id.clone(),
        data_block: "block-10".to_string(),
        point_block_ordinal: 0,
        source_offset: 300,
    };
    let mut second_block_use = block_use.clone();
    second_block_use.id = "nx:feature-history:sketch-named-point-block-use#1-4-1".to_string();
    second_block_use.sketch_reference = "reference-2".to_string();
    second_block_use.reference_ordinal = 1;
    second_block_use.source_offset = 301;

    let groups = feature_sketch_point_groups(std::slice::from_ref(&point));
    let uses = feature_sketch_point_uses(
        &groups,
        std::slice::from_ref(&named_point),
        &[second_block_use.clone(), block_use.clone()],
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].sketch_point_group, groups[0].id);
    assert_eq!(uses[0].named_point, named_point.id);
    assert_eq!(uses[0].sketch_references, ["reference", "reference-2"]);
    assert_eq!(uses[0].block_uses.len(), 2);
    assert_eq!(uses[0].source_offsets, [300, 301]);

    let mut different = point.clone();
    different.id = "different".to_string();
    different.coordinates[1] = f64::from_bits(2.0_f64.to_bits() + 1);
    let different_groups = feature_sketch_point_groups(std::slice::from_ref(&different));
    assert!(feature_sketch_point_uses(
        &different_groups,
        std::slice::from_ref(&named_point),
        std::slice::from_ref(&block_use),
    )
    .is_empty());
    let mut duplicate = point.clone();
    duplicate.id = "payload-point-2".to_string();
    let duplicate_groups = feature_sketch_point_groups(&[point.clone(), duplicate.clone()]);
    assert_eq!(duplicate_groups[0].points, [point.id.clone(), duplicate.id]);
    let uses = feature_sketch_point_uses(
        &duplicate_groups,
        std::slice::from_ref(&named_point),
        std::slice::from_ref(&block_use),
    );
    assert_eq!(uses[0].sketch_point_group, duplicate_groups[0].id);
    let conflicting_groups = feature_sketch_point_groups(&[point, different]);
    assert!(conflicting_groups.is_empty());
    assert!(
        feature_sketch_point_uses(&conflicting_groups, &[named_point], &[block_use]).is_empty()
    );
}

#[test]
fn sketch_point_blocks_establish_ordered_datum_csys_dependencies() {
    use crate::native::{
        FeatureDatumCsysConstruction, FeatureOperationLabel, FeatureSketchDatumCsysBlockRelation,
        FeatureSketchPointUse, OffsetStoreNamedPoint,
    };

    let label = |id: &str, ordinal| FeatureOperationLabel {
        id: id.to_string(),
        section_link: "section".to_string(),
        ordinal,
        value: if ordinal == 0 { "SKETCH" } else { "DATUM_CSYS" }.to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        source_offset: 100 + u64::from(ordinal),
    };
    let labels = [label("sketch", 0), label("csys", 1)];
    let point = OffsetStoreNamedPoint {
        id: "point".to_string(),
        name: "Point1".to_string(),
        data_blocks: vec!["point-first".to_string(), "shared".to_string()],
        values: [1.0, 2.0],
        raw_values: [shifted_f64_bytes(1.0), shifted_f64_bytes(2.0)],
        value_source_offsets: [200, 220],
        source_offset: 190,
    };
    let point_use = FeatureSketchPointUse {
        id: "point-use".to_string(),
        operation_label: "sketch".to_string(),
        sketch_references: vec!["reference".to_string()],
        block_uses: vec!["block-use".to_string()],
        sketch_point_group: "point-group".to_string(),
        named_point: point.id.clone(),
        source_offsets: vec![300],
    };
    let mut blocks = std::array::from_fn(|index| format!("block-{index}"));
    blocks[3] = "shared".to_string();
    let construction = FeatureDatumCsysConstruction {
        id: "construction".to_string(),
        operation_label: "csys".to_string(),
        control: 19,
        object_indices: [0; 8],
        raw_object_indices: std::array::from_fn(|_| vec![0]),
        data_blocks: blocks,
        source_offsets: [400; 8],
    };

    let dependencies = crate::native::feature_sketch_datum_csys_dependencies(
        &labels,
        std::slice::from_ref(&point),
        std::slice::from_ref(&point_use),
        std::slice::from_ref(&construction),
    );
    assert_eq!(dependencies[0].datum_csys_operation_label, "csys");
    assert_eq!(dependencies[0].sketch_operation_label, "sketch");
    assert_eq!(dependencies[0].sketch_point_use, "point-use");
    assert_eq!(
        dependencies[0].block_relation,
        FeatureSketchDatumCsysBlockRelation::Shared {
            data_block: "shared".to_string()
        }
    );

    let consecutive_point = OffsetStoreNamedPoint {
        id: "consecutive-point".to_string(),
        name: "Point2".to_string(),
        data_blocks: vec![
            "nx:om:offset-store#7:block#10".to_string(),
            "nx:om:offset-store#7:block#11".to_string(),
        ],
        values: [3.0, 4.0],
        raw_values: [shifted_f64_bytes(3.0), shifted_f64_bytes(4.0)],
        value_source_offsets: [500, 520],
        source_offset: 490,
    };
    let consecutive_use = FeatureSketchPointUse {
        id: "consecutive-use".to_string(),
        named_point: consecutive_point.id.clone(),
        ..point_use.clone()
    };
    let mut consecutive_construction = construction.clone();
    consecutive_construction.id = "consecutive-construction".to_string();
    consecutive_construction.data_blocks[0] = "nx:om:offset-store#7:block#12".to_string();
    let consecutive_dependencies = crate::native::feature_sketch_datum_csys_dependencies(
        &labels,
        &[consecutive_point],
        &[consecutive_use],
        &[consecutive_construction],
    );
    assert_eq!(
        consecutive_dependencies[0].block_relation,
        FeatureSketchDatumCsysBlockRelation::Consecutive {
            point_data_block: "nx:om:offset-store#7:block#11".to_string(),
            construction_data_block: "nx:om:offset-store#7:block#12".to_string(),
        }
    );

    let mut ambiguous_point = point.clone();
    ambiguous_point.id = "ambiguous-point".to_string();
    let ambiguous_use = FeatureSketchPointUse {
        id: "ambiguous-use".to_string(),
        named_point: ambiguous_point.id.clone(),
        ..point_use.clone()
    };
    assert!(crate::native::feature_sketch_datum_csys_dependencies(
        &labels,
        &[point.clone(), ambiguous_point],
        &[point_use.clone(), ambiguous_use],
        std::slice::from_ref(&construction),
    )
    .is_empty());

    let reversed_labels = [label("csys", 0), label("sketch", 1)];
    assert!(crate::native::feature_sketch_datum_csys_dependencies(
        &reversed_labels,
        &[point],
        &[point_use],
        &[construction],
    )
    .is_empty());
}

#[test]
fn nx_sketch_point_names_require_positive_decimal_suffixes() {
    assert_eq!(crate::native::parse_sketch_point_name("Point1"), Some(1));
    assert_eq!(
        crate::native::parse_sketch_point_name("Point2048"),
        Some(2048)
    );
    for malformed in ["Point", "Point0", "point1", "Point-1", "Point1A"] {
        assert_eq!(crate::native::parse_sketch_point_name(malformed), None);
    }
}

#[test]
fn om_datum_csys_scalar_field_uses_the_common_shifted_binary64_frame() {
    let mut shifted = 25.4_f64.to_be_bytes();
    shifted[0] -= 0x10;
    let mut payload = vec![0xaa, 0x50, 0x59, 0x66, 0x64, 0x00];
    payload.extend_from_slice(&shifted);
    payload.push(0xbb);

    let fields = crate::om::construction_payload_scalar_fields(&payload);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].offset, 1);
    assert_eq!(fields[0].field_code, 0x64);
    assert_eq!(fields[0].value, 25.4);
    assert_eq!(fields[0].raw_value, shifted);
}

#[test]
fn om_simple_hole_lane_requires_two_identical_nonempty_scalar_runs() {
    let shifted = |value: f64| {
        let mut bytes = value.to_be_bytes();
        bytes[0] -= 0x10;
        bytes
    };
    let mut payload = Vec::new();
    for value in [508.0, 38.1, 508.0, 38.1] {
        payload.extend_from_slice(&shifted(value));
        payload.push(0x7f);
    }
    payload.extend_from_slice(&[0x04, 0x08]);
    payload.extend_from_slice(b"Hole_X");
    payload.push(0x00);
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 120,
        value: "SIMPLE HOLE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let lane = crate::om::simple_hole_repeated_scalar_lane(record).unwrap();
    assert_eq!(lane.values[0], 508.0);
    assert!((lane.values[1] - 38.1).abs() < 2.0e-12);
    assert_eq!(lane.raw_values, [shifted(508.0), shifted(38.1)]);
    assert_eq!(lane.witness_offsets, [vec![200, 209], vec![218, 227]]);

    let mut mismatched = payload.clone();
    mismatched[18 + 7] ^= 1;
    assert!(
        crate::om::simple_hole_repeated_scalar_lane(crate::om::OperationRecord {
            bytes: &mismatched,
            payload: &mismatched,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_simple_hole_lane_accepts_one_repeated_scalar() {
    let mut scalar = 25.4f64.to_be_bytes();
    scalar[0] -= 0x10;
    let mut payload = scalar.to_vec();
    payload.push(0x7f);
    payload.extend_from_slice(&scalar);
    payload.extend_from_slice(&[0x04, 0x08]);
    payload.extend_from_slice(b"Hole_X\0");
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label: crate::om::OperationLabel {
            header_offset: 100,
            offset: 120,
            value: "SIMPLE HOLE",
            object_indices: [None; 4],
            object_index_offsets: [0; 4],
        },
    };
    let lane = crate::om::simple_hole_repeated_scalar_lane(record).unwrap();
    assert_eq!(lane.values, [25.4]);
    assert_eq!(lane.raw_values, [scalar]);
    assert_eq!(lane.witness_offsets, [vec![200], vec![209]]);
}

#[test]
fn om_simple_hole_lane_block_references_follow_both_scalar_runs() {
    let shifted = |value: f64| {
        let mut bytes = value.to_be_bytes();
        bytes[0] -= 0x10;
        bytes
    };
    let mut payload = Vec::new();
    payload.extend_from_slice(&shifted(508.0));
    payload.extend_from_slice(&shifted(38.1));
    payload.extend_from_slice(&[0xf0, 0xe7, 0xf0, 0xe8]);
    payload.extend_from_slice(&shifted(508.0));
    payload.extend_from_slice(&shifted(38.1));
    payload.extend_from_slice(&[0xf0, 0xe9, 0xf0, 0xea]);
    payload.extend_from_slice(&[0x04, 0x08]);
    payload.extend_from_slice(b"Hole_X");
    payload.push(0x00);
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 120,
        value: "SIMPLE HOLE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let references = crate::om::simple_hole_repeated_scalar_lane_block_references(record).unwrap();
    assert_eq!(references.first, [231, 232]);
    assert_eq!(references.second, [233, 234]);
    assert_eq!(references.offsets, [[216, 218], [236, 238]]);

    let mut null = payload.clone();
    null[16] = 0xff;
    assert!(
        crate::om::simple_hole_repeated_scalar_lane_block_references(crate::om::OperationRecord {
            bytes: &null,
            payload: &null,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_datum_csys_reference_lane_requires_eight_canonical_indices() {
    let mut payload = vec![
        0x13, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ];
    for value in 42..50 {
        payload.extend_from_slice(&[0xf0, value]);
    }
    payload.extend_from_slice(&[0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]);
    let label = crate::om::OperationLabel {
        header_offset: 10,
        offset: 20,
        value: "DATUM_CSYS",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = crate::om::OperationRecord {
        offset: 10,
        bytes: &payload,
        payload_offset: 100,
        payload: &payload,
        label,
    };
    let field = crate::om::datum_csys_references(record).unwrap();
    assert_eq!(field.control, 0x13);
    assert_eq!(
        field
            .references
            .each_ref()
            .map(|reference| reference.object_index),
        [42, 43, 44, 45, 46, 47, 48, 49]
    );
    assert_eq!(
        field
            .references
            .each_ref()
            .map(|reference| reference.offset),
        [114, 116, 118, 120, 122, 124, 126, 128]
    );
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.raw_object_index.clone())
            .collect::<Vec<_>>(),
        (42..50).map(|value| vec![0xf0, value]).collect::<Vec<_>>()
    );

    let mut alternate_control = payload.clone();
    alternate_control[0] = 0x1a;
    assert_eq!(
        crate::om::datum_csys_references(crate::om::OperationRecord {
            bytes: &alternate_control,
            payload: &alternate_control,
            ..record
        })
        .unwrap()
        .control,
        0x1a
    );

    let mut malformed = payload.clone();
    malformed[14] = 0x2a;
    assert!(
        crate::om::datum_csys_references(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_datum_plane_header_requires_common_prefix_and_nontrivial_count() {
    let payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x29, 0x01, 0x02, 0xf1, 0x02, 0xcf,
    ];
    let label = crate::om::OperationLabel {
        header_offset: 10,
        offset: 20,
        value: "DATUM_PLANE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let record = crate::om::OperationRecord {
        offset: 10,
        bytes: &payload,
        payload_offset: 100,
        payload: &payload,
        label,
    };
    assert_eq!(
        crate::om::datum_plane_payload_header(record),
        Some(crate::om::DatumPlanePayloadHeader {
            control: 0x22,
            declared_count: 3,
            branch_tag: 0x29,
        })
    );
    let mut malformed = payload;
    malformed[6] = 1;
    assert!(
        crate::om::datum_plane_payload_header(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let branch_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x02, 0x23, 0x01, 0x02, 0x80, 0x4c, 0x01, 0xf1, 0x02,
        0xbb, 0x00, 0x14, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x00,
    ];
    let branch = crate::om::datum_plane_single_reference_branch(crate::om::OperationRecord {
        bytes: &branch_payload,
        payload: &branch_payload,
        ..record
    })
    .unwrap();
    assert_eq!(branch.descriptor_index, 76);
    assert_eq!(branch.raw_descriptor_index, [0x80, 0x4c]);
    assert_eq!(branch.descriptor_offset, 110);
    assert_eq!(branch.object_index, 699);
    assert_eq!(branch.raw_object_index, [0xf1, 0x02, 0xbb]);
    assert_eq!(branch.object_offset, 113);

    let double_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x02, 0x29, 0x01, 0x02, 0xf1, 0x02, 0x77, 0x01, 0x01,
        0x18, 0x03, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0xff, 0xf1, 0x02, 0x78, 0x01, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let double = crate::om::datum_plane_double_reference_branch(crate::om::OperationRecord {
        bytes: &double_payload,
        payload: &double_payload,
        ..record
    })
    .unwrap();
    assert_eq!(
        double
            .references
            .each_ref()
            .map(|reference| reference.object_index),
        [631, 632]
    );
    assert_eq!(
        double
            .references
            .each_ref()
            .map(|reference| reference.offset),
        [110, 124]
    );

    let count_three_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x29, 0x01, 0x02, 0xf1, 0x02, 0xcf, 0x01, 0x01,
        0x3a, 0x01, 0x02, 0xf1, 0x02, 0xd0, 0x01, 0x17, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        0xff, 0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let count_three = crate::om::datum_plane_double_reference_branch(crate::om::OperationRecord {
        bytes: &count_three_payload,
        payload: &count_three_payload,
        ..record
    })
    .unwrap();
    assert_eq!(
        count_three
            .references
            .each_ref()
            .map(|reference| reference.object_index),
        [719, 720]
    );
    assert_eq!(
        count_three
            .references
            .each_ref()
            .map(|reference| reference.offset),
        [110, 118]
    );

    let descriptor_count_three_payload = [
        0x22, 0x00, 0x00, 0x01, 0x00, 0x01, 0x03, 0x28, 0x01, 0x02, 0x80, 0x4d, 0x01, 0x29, 0x01,
        0x02, 0xf1, 0x02, 0xd1, 0x01, 0x01, 0x07, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff,
        0xff, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d,
    ];
    let descriptor_count_three =
        crate::om::datum_plane_descriptor_reference_branch(crate::om::OperationRecord {
            bytes: &descriptor_count_three_payload,
            payload: &descriptor_count_three_payload,
            ..record
        })
        .unwrap();
    assert_eq!(descriptor_count_three.descriptor_index, 77);
    assert_eq!(descriptor_count_three.raw_descriptor_index, [0x80, 0x4d]);
    assert_eq!(descriptor_count_three.descriptor_offset, 110);
    assert_eq!(descriptor_count_three.object_index, 721);
    assert_eq!(descriptor_count_three.object_offset, 116);
}

#[test]
fn om_datum_plane_object_index_lane_ends_at_logical_payload_boundary() {
    let bytes = [
        0x80, 0xab, 0x01, 0x04, 0x81, 0x01, 0x01, 0x01, 0x00, 0x12, 0x34, 0x56, 0x78,
    ];
    let lanes = crate::om::datum_plane_object_index_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 2);
    assert_eq!(lanes[0].declared_count, 4);
    assert_eq!(lanes[0].indices, [(257, 4), (1, 6), (1, 7)]);
    assert_eq!(lanes[0].raw_indices, [vec![0x81, 0x01], vec![1], vec![1]]);
    assert_eq!(lanes[0].trailer, 0x1234_5678);

    let mut trailing = bytes.to_vec();
    trailing.push(0);
    assert!(crate::om::datum_plane_object_index_lanes(&trailing).is_empty());
}

#[test]
fn om_datum_plane_object_scalar_pairs_require_the_complete_discriminator() {
    let mut bytes = vec![0x7f, 0x01, 0x01, 0xff];
    bytes.extend_from_slice(&[
        0x6d, 0x00, 0xf0, 0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86,
        0x02, 0x00, 0x03,
    ]);
    bytes.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    bytes.push(0);
    bytes.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    let pairs = crate::om::datum_plane_object_scalar_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].offset, 4);
    assert_eq!(pairs[0].value_offsets, [22, 31]);
    assert_eq!(pairs[0].values, [10.0, -20.0]);
    assert_eq!(pairs[0].raw_values[0], [0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    assert_eq!(pairs[0].raw_values[1], [0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    bytes[10] ^= 1;
    assert!(crate::om::datum_plane_object_scalar_pairs(&bytes).is_empty());
}

#[test]
fn om_datum_plane_descriptor_requires_complete_lowercase_hex_identity() {
    let mut bytes = *b"793487222121a5474a9125451b8e31f5?A\xf0\x1e\xff\x02\x01\x33";
    let descriptor = crate::om::datum_plane_descriptor_block(&bytes).unwrap();
    assert_eq!(descriptor.identity, "793487222121a5474a9125451b8e31f5");
    assert_eq!(descriptor.suffix, b"?A\xf0\x1e\xff\x02\x01\x33");
    assert_eq!(descriptor.schema_index, 28_702);
    assert_eq!(descriptor.label, "3");

    let short_bytes = *b"a75c5f0ed880dd1443b3c5c57908aae?A\xf0\x1f\xff\x02\x01\x66\x33";
    let short = crate::om::datum_plane_descriptor_block(&short_bytes).unwrap();
    assert_eq!(short.identity.len(), 31);
    assert_eq!(short.schema_index, 28_703);
    assert_eq!(short.label, "f3");

    bytes[0] = b'G';
    assert!(crate::om::datum_plane_descriptor_block(&bytes).is_none());
    assert!(crate::om::datum_plane_descriptor_block(&bytes[..39]).is_none());
}

#[test]
fn om_datum_csys_scalar_pairs_require_discriminator_and_separator() {
    let mut bytes = vec![0x2f, 0x2f, 0x41, 0x6d, 0x00, 0xf0];
    bytes.extend_from_slice(&[
        0x08, 0x02, 0x03, 0x01, 0x03, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00, 0x03,
    ]);
    bytes.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    bytes.push(0);
    bytes.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    let pairs = crate::om::object_payload_scalar_pairs(&bytes);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].offset, 6);
    assert_eq!(pairs[0].value_offsets, [21, 30]);
    assert_eq!(pairs[0].values, [10.0, -20.0]);
    assert_eq!(pairs[0].raw_values[0], [0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    assert_eq!(pairs[0].raw_values[1], [0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    assert_eq!(pairs[0].discriminator.len(), 15);

    let mut extended = vec![
        0x08, 0x02, 0x03, 0x01, 0x81, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x00,
        0x03,
    ];
    extended.extend_from_slice(&[0x30, 0x24, 0, 0, 0, 0, 0, 0]);
    extended.push(0);
    extended.extend_from_slice(&[0xb0, 0x34, 0, 0, 0, 0, 0, 0]);
    let extended_pairs = crate::om::object_payload_scalar_pairs(&extended);
    assert_eq!(extended_pairs.len(), 1);
    assert_eq!(extended_pairs[0].discriminator.len(), 16);
    assert_eq!(extended_pairs[0].value_offsets, [16, 25]);
    assert_eq!(
        extended_pairs[0].raw_values[0],
        [0x30, 0x24, 0, 0, 0, 0, 0, 0]
    );

    bytes[29] = 1;
    assert!(crate::om::object_payload_scalar_pairs(&bytes).is_empty());
}

#[test]
fn om_datum_csys_descriptor_requires_one_maximal_hex_identity() {
    let bytes = b"\x02\x01ae166162820ea2d993e1fdf49091850e?A\x80\xa0\xf0\x26";
    let descriptor = crate::om::datum_csys_descriptor_block(bytes).unwrap();
    assert_eq!(descriptor.prefix, [0x02, 0x01]);
    assert_eq!(descriptor.identity, "ae166162820ea2d993e1fdf49091850e");
    assert_eq!(descriptor.identity_offset, 2);
    assert_eq!(descriptor.suffix, b"?A\x80\xa0\xf0\x26");

    let mut ambiguous = bytes.to_vec();
    ambiguous.extend_from_slice(b"012345678901234567890123456789");
    assert!(crate::om::datum_csys_descriptor_block(&ambiguous).is_none());
}

#[test]
fn om_draft_identity_frames_require_complete_typed_framing() {
    let bytes = b"\x00A\x81\x54\xf0\x38\x02\x01abc123?A\xf0\x27\xff\x02\x01def456?\x00";
    let frames = crate::om::draft_construction_identity_frames(bytes);
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].offset, 1);
    assert_eq!(frames[0].prefix, b"A\x81\x54\xf0\x38\x02\x01");
    assert_eq!(
        frames[0].form,
        crate::om::DraftConstructionIdentityFrameForm::IndexedBranch {
            first_index: 340,
            second_index: Some(56),
            branch: 2,
        }
    );
    assert_eq!(frames[0].identity, "abc123");
    assert_eq!(frames[0].identity_offset, 8);
    assert_eq!(frames[1].offset, 15);
    assert_eq!(frames[1].prefix, b"A\xf0\x27\xff\x02\x01");
    assert_eq!(
        frames[1].form,
        crate::om::DraftConstructionIdentityFrameForm::Tagged { index: Some(39) }
    );
    assert_eq!(frames[1].identity, "def456");

    assert!(
        crate::om::draft_construction_identity_frames(b"A\x81\x54\xf0\x38\x02\x01abc123")
            .is_empty()
    );
    assert!(
        crate::om::draft_construction_identity_frames(b"A\x81\x54\xf0\x38\x04\x01abc123?")
            .is_empty()
    );
    assert!(
        crate::om::draft_construction_identity_frames(b"A\xf0\x27\xff\x02\x01ABC123?").is_empty()
    );
}

#[test]
fn om_draft_fixed_lanes_require_complete_discriminator_atoms_and_terminator() {
    let discriminator = [
        0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86, 0x02,
        0x00, 0x01, 0x00,
    ];
    let mut bytes = vec![0xff];
    bytes.extend_from_slice(&discriminator);
    bytes.extend_from_slice(&[0x30, 0x40, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0xb0, 0xc0, 0, 0, 0, 0, 0, 0]);
    bytes.push(0);
    let lanes = crate::om::draft_construction_fixed_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 1);
    assert_eq!(lanes[0].values, [0.5, -0.5]);
    assert_eq!(lanes[0].markers, [0x30, 0xb0]);
    assert_eq!(lanes[0].value_offsets, [19, 27]);

    bytes.pop();
    assert!(crate::om::draft_construction_fixed_lanes(&bytes).is_empty());
    bytes.truncate(22);
    assert!(crate::om::draft_construction_fixed_lanes(&bytes).is_empty());
    assert!(crate::om::draft_construction_fixed_lanes(&discriminator).is_empty());
}

#[test]
fn om_draft_binary32_lanes_require_complete_typed_atoms_and_terminator() {
    let discriminator = [
        0x90, 0x18, 0x45, 0x01, 0x04, 0x01, 0x04, 0x01, 0xc0, 0x45, 0x04, 0x04, 0x80, 0x86, 0x02,
        0x00, 0x03, 0x00,
    ];
    let mut bytes = vec![0xff];
    bytes.extend_from_slice(&discriminator);
    bytes.extend_from_slice(&[0x4f, 0x80, 0, 0]);
    bytes.extend_from_slice(&[0xcf, 0x80, 0, 0]);
    bytes.push(0);
    let lanes = crate::om::draft_construction_binary32_lanes(&bytes);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].offset, 1);
    assert_eq!(lanes[0].discriminator, discriminator);
    assert_eq!(lanes[0].branch, 4);
    assert_eq!(lanes[0].values, [1.0, -1.0]);
    assert_eq!(lanes[0].value_offsets, [19, 23]);

    bytes.pop();
    assert!(crate::om::draft_construction_binary32_lanes(&bytes).is_empty());
    bytes.truncate(21);
    assert!(crate::om::draft_construction_binary32_lanes(&bytes).is_empty());
    assert!(crate::om::draft_construction_binary32_lanes(&discriminator).is_empty());
}

#[test]
fn nx_datum_plane_csys_identity_uses_join_only_equal_typed_identities() {
    let plane = crate::native::FeatureDatumPlaneDescriptor {
        id: "plane-descriptor".into(),
        operation_label: "operation#4".into(),
        datum_plane_header: "plane-header".into(),
        ordinal: 0,
        data_block: "plane-block".into(),
        identity: "012345678901234567890123456789".into(),
        suffix: vec![b'?', b'A'],
        schema_index: 1,
        label: "p".into(),
        source_offset: 10,
    };
    let csys = crate::native::FeatureDatumCsysDescriptor {
        id: "csys-descriptor".into(),
        operation_label: "operation#2".into(),
        construction: "csys-construction".into(),
        reference_ordinal: 7,
        data_block: "csys-block".into(),
        prefix: vec![2, 1],
        identity: plane.identity.clone(),
        suffix: vec![b'?', b'A'],
        source_offset: 20,
        identity_source_offset: 22,
    };
    let uses = crate::native::feature_datum_plane_csys_identity_uses(&[plane], &[csys]);
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].identity, "012345678901234567890123456789");
    assert_eq!(uses[0].datum_plane_operation_label, "operation#4");
    assert_eq!(uses[0].datum_csys_operation_label, "operation#2");
    assert_eq!(uses[0].datum_csys_reference_ordinal, 7);
}

#[test]
fn nx_datum_csys_block_uses_preserve_reference_and_input_order() {
    let construction = crate::native::FeatureDatumCsysConstruction {
        id: "construction".to_string(),
        operation_label: "operation#0".to_string(),
        control: 0x13,
        object_indices: std::array::from_fn(|index| index as u32 + 40),
        raw_object_indices: std::array::from_fn(|index| vec![index as u8 + 40]),
        data_blocks: std::array::from_fn(|index| format!("block#{}", index + 40)),
        source_offsets: std::array::from_fn(|index| index as u64 + 100),
    };
    let input =
        |id: &str, operation: &str, slot: u8, block: &str| crate::native::FeatureInputBlock {
            id: id.to_string(),
            operation_label: operation.to_string(),
            input_slot: slot,
            object_index: 44,
            raw_object_index: vec![44],
            data_block: block.to_string(),
            source_offset: 200,
        };
    let uses = crate::native::feature_datum_csys_block_uses(
        &[construction],
        &[
            input("input#0", "operation#0", 1, "block#43"),
            input("input#1", "operation#6", 0, "block#44"),
            input("input#2", "operation#7", 0, "block#44"),
        ],
    );
    assert_eq!(uses.len(), 3);
    assert_eq!(
        uses[0].id,
        "nx:feature-history:datum-csys-block-use#0-3-0-1"
    );
    assert_eq!(uses[0].reference_ordinal, 3);
    assert_eq!(uses[0].input_operation_label, "operation#0");
    assert_eq!(uses[1].reference_ordinal, 4);
    assert_eq!(uses[1].input_operation_label, "operation#6");
    assert_eq!(uses[2].reference_ordinal, 4);
    assert_eq!(uses[2].input_operation_label, "operation#7");
}

#[test]
fn nx_construction_dependency_requires_a_preceding_projected_operation() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::features::FeatureId;

    let positions = BTreeMap::from([("csys", 1), ("consumer", 2), ("later", 3)]);
    let features = BTreeMap::from([
        ("csys", FeatureId("nx:test:feature#csys".into())),
        ("consumer", FeatureId("nx:test:feature#consumer".into())),
    ]);

    assert_eq!(
        crate::native::preceding_operation_dependency("csys", 2, &positions, &features),
        Some(FeatureId("nx:test:feature#csys".into()))
    );
    assert_eq!(
        crate::native::preceding_operation_dependency("consumer", 2, &positions, &features),
        None
    );
    assert_eq!(
        crate::native::preceding_operation_dependency("later", 2, &positions, &features),
        None
    );
    assert_eq!(
        crate::native::preceding_operation_dependency("missing", 2, &positions, &features),
        None
    );
}

#[test]
fn om_operation_primary_body_reference_requires_one_complete_field() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 100,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [0; 4],
    };
    let bytes = [0x01, 0x02, 0x10, 0x90, 0x19, 0x42, 0xff];
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &bytes,
        payload_offset: 100,
        payload: &bytes,
        label,
    };
    assert_eq!(
        crate::om::operation_body_reference(record),
        Some(crate::om::OperationBodyReference {
            offset: 103,
            object_index: 6466,
            raw_object_index: vec![0x90, 0x19, 0x42],
        })
    );

    let duplicate = [bytes.as_slice(), bytes.as_slice()].concat();
    assert_eq!(
        crate::om::operation_body_references(crate::om::OperationRecord {
            offset: 100,
            bytes: &duplicate,
            payload_offset: 100,
            payload: &duplicate,
            label,
        }),
        [
            crate::om::OperationBodyReference {
                offset: 103,
                object_index: 6466,
                raw_object_index: vec![0x90, 0x19, 0x42],
            },
            crate::om::OperationBodyReference {
                offset: 110,
                object_index: 6466,
                raw_object_index: vec![0x90, 0x19, 0x42],
            },
        ]
    );
    assert!(
        crate::om::operation_body_reference(crate::om::OperationRecord {
            offset: 100,
            bytes: &duplicate,
            payload_offset: 100,
            payload: &duplicate,
            label,
        })
        .is_none()
    );
}

#[test]
fn om_data_block_object_references_require_complete_field_frames() {
    let bytes = [
        0x04, 0x00, 0x2a, 0x02, 0x0b, 0xff, 0x04, 0x00, 0x80, 0xc9, 0x02, 0x0b, 0x04, 0x00, 0x90,
        0x19, 0x42, 0x02, 0x0b,
    ];
    assert_eq!(
        crate::om::data_block_object_references(&bytes),
        [
            crate::om::DataBlockObjectReference {
                offset: 2,
                object_index: 42,
                raw_object_index: vec![0x2a],
            },
            crate::om::DataBlockObjectReference {
                offset: 8,
                object_index: 201,
                raw_object_index: vec![0x80, 0xc9],
            },
            crate::om::DataBlockObjectReference {
                offset: 14,
                object_index: 6466,
                raw_object_index: vec![0x90, 0x19, 0x42],
            },
        ]
    );
    assert_eq!(
        crate::om::data_block_object_references(&bytes[..bytes.len() - 1]).len(),
        2
    );
}

#[test]
fn feature_body_lineage_excludes_tools_consumed_after_their_latest_writer() {
    use crate::native::{
        FeatureBodyReference, FeatureBooleanKind, FeatureBooleanOperation, FeatureOperationLabel,
    };

    let label = |ordinal: u32, value: &str| FeatureOperationLabel {
        id: format!("operation#{ordinal}"),
        section_link: "history#0".to_string(),
        ordinal,
        value: value.to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        source_offset: ordinal as u64,
    };
    let labels = [label(0, "EXTRUDE"), label(1, "EXTRUDE"), label(2, "UNITE")];
    let reference = |operation: &str, body_object_index| FeatureBodyReference {
        id: format!("reference#{body_object_index}"),
        operation_label: operation.to_string(),
        body_object_index,
        raw_body_object_index: vec![body_object_index as u8],
        source_offset: 0,
    };
    let references = [reference("operation#0", 10), reference("operation#1", 20)];
    let booleans = [FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#2".to_string(),
        kind: FeatureBooleanKind::Unite,
        target_object_index: 10,
        raw_target_object_index: vec![10],
        target_source_offset: 0,
        tool_object_indices: vec![20],
        raw_tool_object_indices: vec![vec![20]],
        tool_source_offsets: vec![0],
        source_offset: 0,
    }];

    assert_eq!(
        crate::native::terminal_feature_body_indices(&labels, &references, &booleans, &[], &[]),
        Some([10].into_iter().collect())
    );
}

#[test]
fn feature_body_lineage_consumes_delete_body_references() {
    use crate::native::{FeatureBodyReference, FeatureOperationLabel, SegmentBodyBinding};

    let labels = [FeatureOperationLabel {
        id: "operation#delete".to_string(),
        section_link: "history#0".to_string(),
        ordinal: 0,
        value: "DELETE".to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        source_offset: 0,
    }];
    let references = [FeatureBodyReference {
        id: "reference#10".to_string(),
        operation_label: "operation#delete".to_string(),
        body_object_index: 10,
        raw_body_object_index: vec![10],
        source_offset: 0,
    }];
    let bindings = [SegmentBodyBinding {
        id: "binding#0".to_string(),
        stream_link: "stream#0".to_string(),
        stream_ordinal: 0,
        stream_kind: "partition".to_string(),
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 19,
        source_offset: 0,
    }];

    assert_eq!(
        crate::native::terminal_feature_body_indices(&labels, &references, &[], &[], &bindings,),
        Some(std::collections::BTreeSet::new())
    );
}

#[test]
fn feature_body_lineage_allows_a_writer_after_delete() {
    use crate::native::{FeatureBodyReference, FeatureOperationLabel, SegmentBodyBinding};

    let label = |ordinal: u32, value: &str| FeatureOperationLabel {
        id: format!("operation#{ordinal}"),
        section_link: "history#0".to_string(),
        ordinal,
        value: value.to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        source_offset: u64::from(ordinal),
    };
    let labels = [label(0, "DELETE"), label(1, "EXTRUDE")];
    let reference = |ordinal: u32| FeatureBodyReference {
        id: format!("reference#{ordinal}"),
        operation_label: format!("operation#{ordinal}"),
        body_object_index: 10,
        raw_body_object_index: vec![10],
        source_offset: u64::from(ordinal),
    };
    let references = [reference(0), reference(1)];
    let bindings = [SegmentBodyBinding {
        id: "binding#0".to_string(),
        stream_link: "stream#0".to_string(),
        stream_ordinal: 0,
        stream_kind: "partition".to_string(),
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 19,
        source_offset: 0,
    }];

    assert_eq!(
        crate::native::terminal_feature_body_indices(&labels, &references, &[], &[], &bindings,),
        Some([10, 11].into_iter().collect())
    );
}

#[test]
fn feature_body_lineage_continues_across_ordered_history_sections() {
    use crate::native::{
        FeatureBodyReference, FeatureBooleanKind, FeatureBooleanOperation, FeatureOperationLabel,
    };

    let label = |id: &str, section_link: &str, ordinal, value: &str| FeatureOperationLabel {
        id: id.to_string(),
        section_link: section_link.to_string(),
        ordinal,
        value: value.to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        source_offset: u64::from(ordinal),
    };
    let labels = [
        label("operation#early", "history#0", 0, "EXTRUDE"),
        label("operation#late", "history#1", 0, "UNITE"),
    ];
    let references = [FeatureBodyReference {
        id: "reference#20".to_string(),
        operation_label: "operation#early".to_string(),
        body_object_index: 20,
        raw_body_object_index: vec![20],
        source_offset: 0,
    }];
    let booleans = [FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#late".to_string(),
        kind: FeatureBooleanKind::Unite,
        target_object_index: 10,
        raw_target_object_index: vec![10],
        target_source_offset: 1,
        tool_object_indices: vec![20],
        raw_tool_object_indices: vec![vec![20]],
        tool_source_offsets: vec![1],
        source_offset: 1,
    }];

    assert_eq!(
        crate::native::terminal_feature_body_indices(&labels, &references, &booleans, &[], &[],),
        Some(std::collections::BTreeSet::new())
    );
}

#[test]
fn segment_body_lineage_statuses_cover_every_bound_image() {
    use crate::native::{
        segment_body_lineage_statuses, FeatureBodyReference, FeatureBooleanKind,
        FeatureBooleanOperation, FeatureOperationLabel, SegmentBodyBinding,
    };
    let labels = [
        FeatureOperationLabel {
            id: "operation#0".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 0,
            value: "EXTRUDE".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 0,
        },
        FeatureOperationLabel {
            id: "operation#1".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 1,
            value: "UNITE".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 1,
        },
    ];
    let references = [FeatureBodyReference {
        id: "reference#0".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 10,
        raw_body_object_index: vec![10],
        source_offset: 0,
    }];
    let booleans = [FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#1".to_string(),
        kind: FeatureBooleanKind::Unite,
        target_object_index: 10,
        raw_target_object_index: vec![10],
        target_source_offset: 1,
        tool_object_indices: vec![21],
        raw_tool_object_indices: vec![vec![21]],
        tool_source_offsets: vec![1],
        source_offset: 1,
    }];
    let binding =
        |id: &str, stream_ordinal: u32, stream_kind: &str, body, alias| SegmentBodyBinding {
            id: id.to_string(),
            stream_link: format!("stream#{stream_ordinal}"),
            stream_ordinal,
            stream_kind: stream_kind.to_string(),
            body_object_index: body,
            body_alias_object_index: alias,
            stream_role: 19,
            source_offset: u64::from(stream_ordinal),
        };
    let statuses = segment_body_lineage_statuses(
        &labels,
        &references,
        &booleans,
        &[],
        &[
            binding("binding#0", 0, "partition", 10, 11),
            binding("binding#1", 1, "plain", 20, 21),
        ],
    )
    .unwrap();
    assert_eq!(statuses.len(), 2);
    assert!(statuses[0].terminal);
    assert!(!statuses[1].terminal);
}

#[test]
fn feature_body_segment_uses_require_one_alias_pair() {
    use crate::native::{feature_body_segment_uses, FeatureBodyReference, SegmentBodyBinding};
    let reference = FeatureBodyReference {
        id: "nx:feature-history:body-reference#0".into(),
        operation_label: "operation#0".into(),
        body_object_index: 11,
        raw_body_object_index: vec![11],
        source_offset: 90,
    };
    let binding = SegmentBodyBinding {
        id: "binding#0".into(),
        stream_link: "stream#3".into(),
        stream_ordinal: 3,
        stream_kind: "plain".into(),
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 19,
        source_offset: 40,
    };
    let uses = feature_body_segment_uses(&[reference.clone()], &[binding.clone()]);
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].feature_body_reference, reference.id);
    assert_eq!(uses[0].segment_body_binding, binding.id);
    assert!(feature_body_segment_uses(&[reference], &[binding.clone(), binding]).is_empty());
}

#[test]
fn feature_body_lineage_treats_segment_tuple_indices_as_one_identity() {
    use crate::native::{
        FeatureBodyReference, FeatureBooleanKind, FeatureBooleanOperation, FeatureOperationLabel,
        SegmentBodyBinding,
    };

    let label = |ordinal: u32, value: &str| FeatureOperationLabel {
        id: format!("operation#{ordinal}"),
        section_link: "history#0".to_string(),
        ordinal,
        value: value.to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        source_offset: ordinal as u64,
    };
    let labels = [label(0, "EXTRUDE"), label(1, "UNITE")];
    let references = [FeatureBodyReference {
        id: "reference#150".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 150,
        raw_body_object_index: vec![0x80, 150],
        source_offset: 0,
    }];
    let booleans = [FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#1".to_string(),
        kind: FeatureBooleanKind::Unite,
        target_object_index: 10,
        raw_target_object_index: vec![10],
        target_source_offset: 0,
        tool_object_indices: vec![94],
        raw_tool_object_indices: vec![vec![94]],
        tool_source_offsets: vec![0],
        source_offset: 0,
    }];
    let bindings = [SegmentBodyBinding {
        id: "binding#0".to_string(),
        stream_link: "stream#0".to_string(),
        stream_ordinal: 0,
        stream_kind: "partition".to_string(),
        body_object_index: 94,
        body_alias_object_index: 150,
        stream_role: 19,
        source_offset: 0,
    }];

    assert_eq!(
        crate::native::terminal_feature_body_indices(
            &labels,
            &references,
            &booleans,
            &[],
            &bindings,
        ),
        Some(std::collections::BTreeSet::new())
    );
}

#[test]
fn feature_body_lineage_closes_overlapping_alias_pairs_transitively() {
    use crate::native::{
        segment_body_lineage_statuses, FeatureBodyReference, FeatureBooleanKind,
        FeatureBooleanOperation, FeatureOperationLabel, SegmentBodyBinding,
    };

    let label = |ordinal: u32, value: &str| FeatureOperationLabel {
        id: format!("operation#{ordinal}"),
        section_link: "history#0".to_string(),
        ordinal,
        value: value.to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        source_offset: u64::from(ordinal),
    };
    let labels = [label(0, "EXTRUDE"), label(1, "UNITE")];
    let references = [FeatureBodyReference {
        id: "reference#30".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 30,
        raw_body_object_index: vec![30],
        source_offset: 0,
    }];
    let booleans = [FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#1".to_string(),
        kind: FeatureBooleanKind::Unite,
        target_object_index: 99,
        raw_target_object_index: vec![99],
        target_source_offset: 1,
        tool_object_indices: vec![10],
        raw_tool_object_indices: vec![vec![10]],
        tool_source_offsets: vec![1],
        source_offset: 1,
    }];
    let binding = |id: &str, stream_ordinal, body, alias| SegmentBodyBinding {
        id: id.to_string(),
        stream_link: format!("stream#{stream_ordinal}"),
        stream_ordinal,
        stream_kind: "partition".to_string(),
        body_object_index: body,
        body_alias_object_index: alias,
        stream_role: 19,
        source_offset: u64::from(stream_ordinal),
    };
    let bindings = [
        binding("binding#0", 0, 10, 20),
        binding("binding#1", 1, 30, 20),
        binding("binding#2", 2, 40, 20),
    ];

    let statuses =
        segment_body_lineage_statuses(&labels, &references, &booleans, &[], &bindings).unwrap();
    assert_eq!(statuses.len(), 3);
    assert!(statuses.iter().all(|status| !status.terminal));
}

#[test]
fn feature_body_lineage_consumes_segment_bound_sew_operands() {
    use crate::native::{FeatureOperationBodyOperand, FeatureOperationLabel, SegmentBodyBinding};
    let labels = [FeatureOperationLabel {
        id: "operation#0".to_string(),
        section_link: "history#0".to_string(),
        ordinal: 0,
        value: "SEW".to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        source_offset: 0,
    }];
    let bindings = [SegmentBodyBinding {
        id: "binding#0".to_string(),
        stream_link: "stream#0".to_string(),
        stream_ordinal: 0,
        stream_kind: "partition".to_string(),
        body_object_index: 20,
        body_alias_object_index: 30,
        stream_role: 0,
        source_offset: 0,
    }];
    let operands = [FeatureOperationBodyOperand {
        id: "operand#0".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 10,
        body_reference_ordinal: 0,
        ordinal: 0,
        operand_object_index: 30,
        raw_operand_object_index: vec![30],
        segment_body_bindings: vec!["binding#0".to_string()],
        source_offset: 0,
    }];
    assert_eq!(
        crate::native::terminal_feature_body_indices(&labels, &[], &[], &operands, &bindings),
        Some(std::collections::BTreeSet::new())
    );
}

#[test]
fn om_size_frame_bounds_its_type_declarations() {
    let bytes = size_framed_om_section();
    let sections = crate::om::sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].offset, 0);
    assert_eq!(sections[0].byte_len, bytes.len());
    assert_eq!(sections[0].types.len(), 2);
    assert_eq!(sections[0].types[0].name, "UGS::FEATURE_RECORD");
    assert_eq!(
        sections[0].types[0].registry_suffix,
        &[0x81, 0x21, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x06]
    );
    assert_eq!(sections[0].types[1].trailing_code, 0x65);
    assert_eq!(sections[0].fields.len(), 2);
    assert_eq!(sections[0].fields[0].name, "m_target");
    assert_eq!(sections[0].fields[1].trailing_code, 0x81);
    assert_eq!(sections[0].record_area, None);

    let mut truncated = bytes;
    truncated.pop();
    assert!(crate::om::sections(&truncated).is_empty());
}

#[test]
fn om_size_frame_uses_validated_internal_record_area_pointer() {
    let bytes = size_framed_om_section_with_record_area();
    let section = crate::om::sections(&bytes).remove(0);
    let offset = section.record_area_offset.expect("record area");
    assert_eq!(offset, size_framed_om_section().len() + 20);
    assert_eq!(section.record_area.unwrap(), &bytes[offset..]);
    assert_eq!(&bytes[offset + 12..offset + 15], &[0x05, 0x01, 0x0e]);

    let mut invalid = bytes;
    invalid[offset + 12] = 1;
    assert_eq!(crate::om::sections(&invalid)[0].record_area, None);
}

#[test]
fn om_operation_labels_require_the_complete_frame() {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x01\x82\x40\x90\x17\xd3\xff\x03\x07UNITE\0\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\x02\x03\xff\xff\x03\x08SKETCH\0";
    let labels = crate::om::operation_labels(bytes, 100);
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0].offset, 122);
    assert_eq!(labels[0].header_offset, 100);
    assert_eq!(labels[0].value, "UNITE");
    assert_eq!(
        labels[0].object_indices,
        [Some(1), Some(576), Some(6099), None]
    );
    assert_eq!(labels[1].value, "SKETCH");
    assert_eq!(labels[1].object_indices, [Some(2), Some(3), None, None]);

    assert!(crate::om::operation_labels(b"\xff\xff\x03\x07UNITE\0", 0).is_empty());
    let mut invalid = bytes.to_vec();
    invalid[15] = 0x91;
    assert_eq!(crate::om::operation_labels(&invalid, 0).len(), 1);
}

#[test]
fn om_operation_records_use_consecutive_validated_headers() {
    let bytes = b"prefix\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x07UNITE\0payload\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x08SKETCH\0tail";
    let records = crate::om::operation_records(bytes, 10);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].offset, 16);
    assert_eq!(records[0].label.value, "UNITE");
    assert!(records[0].bytes.ends_with(b"payload"));
    assert_eq!(records[0].payload, b"payload");
    assert_eq!(records[0].payload_offset, 43);
    assert_eq!(records[1].label.value, "SKETCH");
    assert!(records[1].bytes.ends_with(b"tail"));
    assert_eq!(records[1].payload, b"tail");
}

#[test]
fn om_operation_payload_strings_require_complete_utf8_frames() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SIMPLE HOLE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x00\x04\x07BLOCK\0\x04\x04\xc3\x97\0\x04\x07BROKEN";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let strings = crate::om::operation_payload_strings(record);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[0].offset, 201);
    assert_eq!(strings[0].value, "BLOCK");
    assert_eq!(strings[1].value, "×");
}

#[test]
fn om_surface_payload_strings_require_exact_length_utf8_and_terminator() {
    let bytes = b"\x66\x1b\x03\x05Steel\0\xaa\x66\x1b\x03\x02\xc3\x97\0";
    let strings = crate::om::surface_payload_strings(bytes);
    assert_eq!(strings.len(), 2);
    assert_eq!(strings[0].offset, 0);
    assert_eq!(strings[0].value, "Steel");
    assert_eq!(strings[1].offset, 11);
    assert_eq!(strings[1].value, "×");

    let truncated = b"\x66\x1b\x03\x05Steel";
    assert!(crate::om::surface_payload_strings(truncated).is_empty());
    let invalid_utf8 = b"\x66\x1b\x03\x01\xff\0";
    assert!(crate::om::surface_payload_strings(invalid_utf8).is_empty());
    let control = b"\x66\x1b\x03\x01\n\0";
    assert!(crate::om::surface_payload_strings(control).is_empty());
}

#[test]
fn om_projected_curve_references_require_one_complete_field() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "CPROJ",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload =
        b"\0\x01\x02\xf1\x02\xc8\xf1\x02\xc9\x80\x57\x00\x02\x01\xf1\x02\xca\xff\x01\x02\x02\x7d\0";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = crate::om::projected_curve_payload_references(record).expect("complete field");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| (reference.object_index, reference.offset))
            .collect::<Vec<_>>(),
        [(712, 203), (713, 206), (714, 214)]
    );

    let mut malformed = payload.to_vec();
    malformed[17] = 0x00;
    assert!(
        crate::om::projected_curve_payload_references(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        crate::om::projected_curve_payload_references(crate::om::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_combined_projected_curve_references_require_the_complete_graph() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "CPROJ_CMB",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x3c\x32\x01\x02\x32\x01\x04\x36\x01\x33\xf1\x03\x18\x33\xf1\x03\x19\x00\xf1\x03\x1a\x00\x00\x00\x00\x00\x00\xf1\x03\x1b\x16\x01\x02\xf1\x03\x18\x01\x02\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x03\x1c\x00\x81\x5c\x16\x01\x02\xf1\x03\x19\x01\x02\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x03\x1d\x00\x81\x5c\xff\x01\xff\x01\xf1\x03\x1e\xf1\x03\x1f\x04\x02";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = crate::om::projected_curve_payload_references(record).expect("complete graph");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| (reference.object_index, reference.offset))
            .collect::<Vec<_>>(),
        [
            (792, 210),
            (793, 214),
            (794, 218),
            (795, 227),
            (796, 246),
            (797, 268),
            (798, 278),
            (799, 281),
        ]
    );

    let mut inconsistent = payload.to_vec();
    inconsistent[35] = 0x19;
    assert!(
        crate::om::projected_curve_payload_references(crate::om::OperationRecord {
            bytes: &inconsistent,
            payload: &inconsistent,
            ..record
        })
        .is_none()
    );

    let mut malformed = payload.to_vec();
    malformed[84] = 0x00;
    assert!(
        crate::om::projected_curve_payload_references(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        crate::om::projected_curve_payload_references(crate::om::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_pattern_reference_graph_preserves_nullable_terminal_slot() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "Pattern Geometry",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let nullable = b"\x61\xf1\x1b\x08\xff\x00\xff\x01\xf1\x1b\x09\xf1\x1b\x0a\x61\xf1\x1b\x0b\xff\x00\xff\x01\xf1\x1b\x0c\xf1\x1b\x0d\xff\x62\xf1\x1b\x0e\xf1\x1b\x0f\xff\x00\x00\x01\xf1\x1b\x10\xff\xff\xff\x01";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: nullable,
        payload_offset: 200,
        payload: nullable,
        label,
    };
    let field = crate::om::pattern_payload_references(record).expect("complete graph");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        (6920..=6928).collect::<Vec<_>>()
    );

    let populated = [&nullable[..nullable.len() - 4], b"\xf1\x1b\x11\xff\xff\x01"].concat();
    let field = crate::om::pattern_payload_references(crate::om::OperationRecord {
        label: crate::om::OperationLabel {
            value: "Pattern Feature",
            ..label
        },
        bytes: &populated,
        payload: &populated,
        ..record
    })
    .expect("populated terminal slot");
    assert_eq!(field.references.len(), 10);
    assert_eq!(field.references[9].object_index, 6929);

    let mut malformed = nullable.to_vec();
    malformed[18] = 0x60;
    assert!(
        crate::om::pattern_payload_references(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_pattern_transform_lanes_require_counted_family_rows() {
    let feature_payload = b"\xaa\x01\x03\x60\x01\x00\x00\x50\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\x60\x01\x00\x00\xd0\x54\x00\x00\x00\x01\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x9f\xfe\x01\x02\x00\x00\xff\x00\x00\x5f\x00\x00\x01";
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "Pattern Feature",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: feature_payload,
        payload_offset: 200,
        payload: feature_payload,
        label,
    };
    let lane = crate::om::pattern_payload_transform_lane(record).expect("feature lane");
    assert_eq!(lane.offset, 201);
    assert_eq!(lane.declared_count, 3);
    assert_eq!(lane.encoding, crate::om::PatternTransformEncoding::Binary32);
    assert_eq!(lane.values, [3.3125, -3.3125]);
    assert_eq!(lane.value_offsets, [207, 237]);
    assert_eq!(lane.selectors, [2, 8190]);
    assert_eq!(lane.raw_selectors, [vec![0x02], vec![0x9f, 0xfe]]);
    assert_eq!(lane.selector_offsets, [225, 255]);

    let geometry_payload = b"\x01\x03\x60\x01\x00\x00\x00\x00\x01\x00\x30\x60\x80\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x02\x01\x01\x00\x00\xff\x00\x00\x60\x01\x00\x00\x00\x00\x01\x00\x30\x70\x80\x00\x00\x00\x00\x00\x00\x00\x01\x00\x00\x00\x00\x01\x01\x03\x03\x01\x02\x00\x00\xff\x00\x00\x5f\x00\x00\x01";
    let geometry_record = crate::om::OperationRecord {
        label: crate::om::OperationLabel {
            value: "Pattern Geometry",
            ..label
        },
        bytes: geometry_payload,
        payload: geometry_payload,
        ..record
    };
    let lane = crate::om::pattern_payload_transform_lane(geometry_record).expect("geometry lane");
    assert_eq!(lane.encoding, crate::om::PatternTransformEncoding::Binary64);
    assert_eq!(lane.values, [132.0, 264.0]);
    assert_eq!(lane.selectors, [2, 3]);
    assert_eq!(lane.raw_selectors, [vec![0x02], vec![0x03]]);
    assert_eq!(lane.selector_offsets, [228, 262]);

    let mut wrong_ordinal = feature_payload.to_vec();
    wrong_ordinal[29] = 2;
    assert!(
        crate::om::pattern_payload_transform_lane(crate::om::OperationRecord {
            bytes: &wrong_ordinal,
            payload: &wrong_ordinal,
            ..record
        })
        .is_none()
    );
    assert!(
        crate::om::pattern_payload_transform_lane(crate::om::OperationRecord {
            bytes: &feature_payload[..feature_payload.len() - 1],
            payload: &feature_payload[..feature_payload.len() - 1],
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_geometry_instance_reference_requires_one_complete_field() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "Geometry Instance",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x44\x45\x00\xff\xff\xf1\x03\x21\x01\x02\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x01\x02";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = crate::om::pattern_payload_references(record).expect("complete field");
    assert_eq!(field.references[0].object_index, 801);
    assert_eq!(field.references[0].offset, 205);

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        crate::om::pattern_payload_references(crate::om::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_point_feature_header_requires_the_complete_leading_envelope() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "POINT",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x72\x00\x00\x01\x00\x00\x00\xf1\x1c\x8f\x00\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x0d\x01\x02\x01\x00\x00\x00\x89\x02\x01\x01\x01\x00\xa5\x57\x95\x01\x00\x00\xff\x02\xc0\x1f\xff\xfd\x01\x00\x00\x01\x01\x01\x03\x02\x01\x01\x01\x00\x00\x00\x00\x00\xaa";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let header = crate::om::point_feature_payload_header(record).expect("complete header");
    assert_eq!(header.reference.object_index, 7311);
    assert_eq!(header.reference.offset, 207);
    assert_eq!(header.mode, 0x02);

    let mut alternate_mode = payload.to_vec();
    alternate_mode[52] = 0x03;
    assert_eq!(
        crate::om::point_feature_payload_header(crate::om::OperationRecord {
            bytes: &alternate_mode,
            payload: &alternate_mode,
            ..record
        })
        .expect("alternate mode")
        .mode,
        0x03
    );

    for malformed_offset in [0, 10, 51, 72] {
        let mut malformed = payload.to_vec();
        malformed[malformed_offset] ^= 0x01;
        assert!(
            crate::om::point_feature_payload_header(crate::om::OperationRecord {
                bytes: &malformed,
                payload: &malformed,
                ..record
            })
            .is_none()
        );
    }
    let mut unsupported_mode = payload.to_vec();
    unsupported_mode[52] = 0x04;
    assert!(
        crate::om::point_feature_payload_header(crate::om::OperationRecord {
            bytes: &unsupported_mode,
            payload: &unsupported_mode,
            ..record
        })
        .is_none()
    );
    assert!(
        crate::om::point_feature_payload_header(crate::om::OperationRecord {
            bytes: &payload[..72],
            payload: &payload[..72],
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_point_feature_scalar_lane_spans_the_preceding_block_atomically() {
    let mut encoded = Vec::new();
    for value in [1.0_f64, -2.0, 3.5, 4.0, 5.25, -6.0] {
        let mut bytes = value.to_be_bytes();
        bytes[0] -= 0x10;
        encoded.extend_from_slice(&bytes);
    }
    let preceding = [vec![0xaa, 0xbb], encoded[..3].to_vec()].concat();
    let mut target = encoded[3..].to_vec();
    target.extend_from_slice(&[
        0x00, 0x25, 0x25, 0x41, 0x00, 0x04, 0x01, 0x07, 0x01, 0xc0, 0x45, 0x10, 0x00, 0x80, 0x86,
        0x02, 0x00, 0x01, 0x00,
    ]);
    target.push(0xcc);

    let lane = crate::om::point_feature_scalar_lane(&preceding, &target).expect("complete lane");
    assert_eq!(lane.values, [1.0, -2.0, 3.5, 4.0, 5.25, -6.0]);
    assert_eq!(lane.raw_values.concat(), encoded);
    assert_eq!(lane.value_offsets, [2, 10, 18, 26, 34, 42]);

    let mut malformed = target.clone();
    malformed[45] = 0x01;
    assert!(crate::om::point_feature_scalar_lane(&preceding, &malformed).is_none());
    assert!(crate::om::point_feature_scalar_lane(&preceding[..2], &target).is_none());
    assert!(crate::om::point_feature_scalar_lane(&preceding, &target[..63]).is_none());

    let mut nonfinite = target;
    nonfinite[5..13].copy_from_slice(&[0x6f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    assert!(crate::om::point_feature_scalar_lane(&preceding, &nonfinite).is_none());
}

#[test]
fn om_draft_feature_references_require_one_complete_graph() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "DRAFT",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let prefix = b"\x67\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\xff\xff\xff\xff\xff\xff\xff\xff\x01\x03\x80\x94\x82\x49";
    let graph = b"\x01\x02\xf1\x1b\x7c\x01\x02\xf1\x1b\x7d\x68\x2f\x70\x62\x4d\xd2\xf1\xa9\xfc\x03\x50\x44\x00\x00\x01\x46\x8a\x2a\x01\xa3\x60\x10\x01\x01\x01\x04\x02\x01\x02\x01\x00\x00\x00\x00\x01\xf1\x1b\x7e\xff\x00\x00\x00\xf1\x1b\x7f\xff";
    let terminal = b"\x81\x5e\x80\xb8\x01\x03\x02\x01\x02\x01\x01\x01\x00\x00\x00\x29\x29\x0c\x00";
    let payload = [prefix.as_slice(), graph.as_slice(), terminal.as_slice()].concat();
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let field = crate::om::draft_feature_payload_references(record).expect("complete graph");
    assert_eq!(
        field
            .references
            .clone()
            .map(|reference| reference.object_index),
        [7036, 7037, 7038, 7039]
    );
    assert_eq!(
        field.references.map(|reference| reference.offset),
        [230, 235, 273, 280]
    );
    let lane = crate::om::draft_feature_leading_index_lane(record).expect("complete index lane");
    assert_eq!(lane.declared_count, 3);
    assert_eq!(lane.indices, vec![(148, 224), (585, 226)]);
    assert_eq!(lane.raw_indices, vec![vec![0x80, 0x94], vec![0x82, 0x49]]);
    let terminal_lane =
        crate::om::draft_feature_terminal_lane(record).expect("complete terminal lane");
    assert_eq!(terminal_lane.indices, [350, 184]);
    assert_eq!(terminal_lane.raw_indices, [[0x81, 0x5e], [0x80, 0xb8]]);
    assert_eq!(terminal_lane.index_offsets, [284, 286]);
    assert_eq!(terminal_lane.tail, [0x29, 0x29, 0x0c]);
    assert_eq!(terminal_lane.offset, 284);

    let mut malformed = payload.clone();
    malformed[53] = 0x00;
    assert!(
        crate::om::draft_feature_payload_references(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );
    let mut malformed_lane = payload.clone();
    malformed_lane[23] = 4;
    assert!(
        crate::om::draft_feature_leading_index_lane(crate::om::OperationRecord {
            bytes: &malformed_lane,
            payload: &malformed_lane,
            ..record
        })
        .is_none()
    );
    let ambiguous = [prefix.as_slice(), graph.as_slice(), graph.as_slice()].concat();
    assert!(
        crate::om::draft_feature_payload_references(crate::om::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
    assert!(
        crate::om::draft_feature_payload_references(crate::om::OperationRecord {
            bytes: &payload[..prefix.len() + graph.len() - 2],
            payload: &payload[..prefix.len() + graph.len() - 2],
            ..record
        })
        .is_none()
    );
    assert!(
        crate::om::draft_feature_terminal_lane(crate::om::OperationRecord {
            bytes: &payload[..payload.len() - 1],
            payload: &payload[..payload.len() - 1],
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_surface_feature_references_require_the_complete_common_envelope() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SKIN",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x3f\x00\x00\x01\x00\xf1\x02\x46\xf1\x02\x47\xf1\x02\x48\x01\x09\x03\x03\x04\x05\x02\x01\x01\x01\x01\x09\xf1\x02\x49\xf1\x02\x4a\xf1\x02\x4b\xf1\x02\x4c\xf1\x02\x4d\xf1\x02\x4e\xf1\x02\x4f\xf1\x02\x50\x00\x03\x03\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xf1\x02\x56\xf1\x02\x57\xf1\x02\x58\x01\x01\xff\xff\xff\xff\xff\xff\xff\xff\xff\x00\x00\x00\x00\x01\x02";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = crate::om::surface_feature_payload_references(record).expect("complete envelope");
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        [582, 583, 584, 585, 586, 587, 588, 589, 590, 591, 592, 598, 599, 600,]
    );

    let studio_payload = [&[0x14], &payload[1..]].concat();
    let studio = crate::om::OperationRecord {
        label: crate::om::OperationLabel {
            value: "Studio Surface",
            ..label
        },
        bytes: &studio_payload,
        payload: &studio_payload,
        ..record
    };
    assert!(crate::om::surface_feature_payload_references(studio).is_some());

    let mut malformed = payload.to_vec();
    let last = malformed.len() - 1;
    malformed[last] = 0x00;
    assert!(
        crate::om::surface_feature_payload_references(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), &payload[51..]].concat();
    assert!(
        crate::om::surface_feature_payload_references(crate::om::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_surface_feature_branches_require_one_complete_counted_group() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SKIN",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\xa0\x5a\x14\x13\x01\x02\x40\x01\x04\xf1\x1b\xf4\xf1\x1b\xf5\xf1\x1b\xf6\x01\x04\x00\x00\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x1b\xf7\x00\x81\x58\x01\x02\x40\x01\x05\xf1\x1b\xf8\xf1\x1b\xf9\xf1\x1b\xfa\xf1\x1b\xfb\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x1b\xfc\x00\x81\x1c\x00\x00\x00\x01\x03\x00\x00\x00\xff\xff\x01";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let group = crate::om::surface_feature_payload_branches(record).expect("complete group");
    assert_eq!(group.family, 0x14);
    assert_eq!(group.header_code, 0x13);
    assert_eq!(group.branches.len(), 2);
    assert_eq!(group.branches[0].mode, 0x40);
    assert_eq!(group.branches[0].declared_count, 4);
    assert!(group.branches[0].witnessed);
    assert_eq!(group.branches[0].members.len(), 3);
    assert_eq!(group.branches[0].terminal.object_index, 7159);
    assert_eq!(group.branches[0].suffix, [0x81, 0x58, 0x01, 0x02]);
    assert_eq!(group.branches[1].declared_count, 5);
    assert!(!group.branches[1].witnessed);
    assert_eq!(group.branches[1].members.len(), 4);
    assert_eq!(group.branches[1].terminal.object_index, 7164);
    assert_eq!(group.branches[1].suffix, [0x81, 0x1c]);

    let studio_payload = [
        &payload[..payload.len() - 11],
        &[0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0x01],
    ]
    .concat();
    let studio = crate::om::OperationRecord {
        label: crate::om::OperationLabel {
            value: "Studio Surface",
            ..label
        },
        bytes: &studio_payload,
        payload: &studio_payload,
        ..record
    };
    assert!(crate::om::surface_feature_payload_branches(studio).is_some());

    let mut malformed = payload.to_vec();
    malformed[19] = 0x03;
    assert!(
        crate::om::surface_feature_payload_branches(crate::om::OperationRecord {
            bytes: &malformed,
            payload: &malformed,
            ..record
        })
        .is_none()
    );

    let ambiguous = [payload.as_slice(), payload.as_slice()].concat();
    assert!(
        crate::om::surface_feature_payload_branches(crate::om::OperationRecord {
            bytes: &ambiguous,
            payload: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_sketch_payload_reference_field_is_counted_ordered_and_canonical() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SKETCH",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x00\x01\x05\xf0\xff\xf1\x01\x00\xf1\x01\x01\xf1\x01\x02\x00\x00\xf1\x01\x03\x01\x00\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = crate::om::sketch_payload_references(record).unwrap();
    assert_eq!(field.declared_count, 5);
    let references: [crate::om::PayloadObjectReference; 5] =
        field.references.clone().try_into().unwrap();
    assert_eq!(
        references.clone().map(|reference| reference.object_index),
        [255, 256, 257, 258, 259]
    );
    assert_eq!(
        references.map(|reference| reference.offset),
        [204, 206, 209, 212, 217]
    );
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.raw_object_index.as_slice())
            .collect::<Vec<_>>(),
        [
            &[0xf0, 0xff][..],
            &[0xf1, 0x01, 0x00][..],
            &[0xf1, 0x01, 0x01][..],
            &[0xf1, 0x01, 0x02][..],
            &[0xf1, 0x01, 0x03][..],
        ]
    );
    let zero = b"\x01\x00\x00\x00\x00\xf0\x42\x01\x00\x00\x00";
    let field = crate::om::sketch_payload_references(crate::om::OperationRecord {
        payload: zero,
        bytes: zero,
        ..record
    })
    .unwrap();
    assert_eq!(field.declared_count, 0);
    assert_eq!(field.references.len(), 1);
    assert_eq!(field.references[0].object_index, 0x42);
    let two = b"\x01\x00\x01\x02\xf0\x41\x00\x00\xf0\x42\x01\x00\x00\x00";
    let field = crate::om::sketch_payload_references(crate::om::OperationRecord {
        payload: two,
        bytes: two,
        ..record
    })
    .unwrap();
    assert_eq!(field.declared_count, 2);
    assert_eq!(
        field
            .references
            .iter()
            .map(|reference| reference.object_index)
            .collect::<Vec<_>>(),
        [0x41, 0x42]
    );

    let mut noncanonical = payload.to_vec();
    noncanonical[7] = 0;
    assert!(
        crate::om::sketch_payload_references(crate::om::OperationRecord {
            payload: &noncanonical,
            bytes: &noncanonical,
            ..record
        })
        .is_none()
    );
    assert!(
        crate::om::sketch_payload_references(crate::om::OperationRecord {
            label: crate::om::OperationLabel {
                value: "BLOCK",
                ..label
            },
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_extrude_profile_references_require_matching_witness_field() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x02\x16\x01\x03\xf0\xff\xf1\x01\x00\x01\x03\x79\xaa\x01\x03\xf0\xff\xf1\x01\x00\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let field = crate::om::extrude_profile_references(record).unwrap();
    assert!(field.witnessed);
    let references = field.references;
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].object_index, 255);
    assert_eq!(references[0].raw_object_index, [0xf0, 0xff]);
    assert_eq!(references[0].offset, 205);
    assert_eq!(references[1].object_index, 256);
    assert_eq!(references[1].raw_object_index, [0xf1, 0x01, 0x00]);
    assert_eq!(references[1].offset, 207);

    let without_witness = &payload[..14];
    let field = crate::om::extrude_profile_references(crate::om::OperationRecord {
        payload: without_witness,
        bytes: without_witness,
        ..record
    })
    .unwrap();
    assert!(!field.witnessed);
    assert_eq!(field.references.len(), 2);
    assert!(
        crate::om::extrude_profile_references(crate::om::OperationRecord {
            label: crate::om::OperationLabel {
                value: "SKETCH",
                ..label
            },
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_extrude_header_decodes_shifted_ieee_scalars() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload =
        b"\x0f\x00\x00\x01\x00\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x2f\xa3\x74\xbc\x6a\x7e\xf9\xdb";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let header = crate::om::extrude_payload_header(record).unwrap();
    assert_eq!(header.offset, 205);
    assert_eq!(header.scalars, [0.04, 0.038]);
    assert_eq!(header.raw_scalars.concat(), payload[5..21]);

    let mut invalid = payload.to_vec();
    invalid[5] = 0xf0;
    assert!(
        crate::om::extrude_payload_header(crate::om::OperationRecord {
            payload: &invalid,
            bytes: &invalid,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_extrude_footer_requires_one_complete_terminal_lane() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let payload = b"\x01\x01\x02\x81\x5f\x80\xab\x01\x03\x02\x01\x01\x02\x01\x01\x00\x00\x00\x29\x29\x05\x80\xff\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: payload,
        payload_offset: 200,
        payload,
        label,
    };
    let footer = crate::om::extrude_payload_footer(record).unwrap();
    assert_eq!(footer.offset, 200);
    assert_eq!(footer.type_indices, [351, 171]);
    assert_eq!(
        footer.raw_type_indices,
        [vec![0x81, 0x5f], vec![0x80, 0xab]]
    );
    assert_eq!(footer.type_index_offsets, [203, 205]);
    assert_eq!(footer.mode_indices, [2, 1]);
    assert_eq!(footer.flags, [1, 2, 1, 1]);
    assert_eq!(footer.trailing_indices, [5, 255]);
    assert_eq!(footer.raw_trailing_indices, [vec![0x05], vec![0x80, 0xff]]);
    assert_eq!(footer.trailing_index_offsets, [220, 221]);

    let truncated = &payload[..payload.len() - 1];
    assert!(
        crate::om::extrude_payload_footer(crate::om::OperationRecord {
            payload: truncated,
            bytes: truncated,
            ..record
        })
        .is_none()
    );

    let mut ambiguous = payload[..payload.len() - 1].to_vec();
    ambiguous.extend_from_slice(payload);
    assert!(
        crate::om::extrude_payload_footer(crate::om::OperationRecord {
            payload: &ambiguous,
            bytes: &ambiguous,
            ..record
        })
        .is_none()
    );
}

#[test]
fn om_operation_body_scalar_clauses_preserve_body_order_and_branch() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "TRIM BODY",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x42\xff\x1c\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\xaa\x01\x02\x10\x43\xff\x11\x30\x00\x00\x00\x00\x00\x00\x00\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let triples = crate::om::operation_body_scalar_triples(record);
    assert_eq!(triples.len(), 2);
    assert_eq!(triples[0].body_reference_ordinal, 0);
    assert_eq!(triples[0].body_object_index, 66);
    assert_eq!(triples[0].branch, 0x1c);
    assert_eq!(
        triples[0].scalars.each_ref().map(|scalar| scalar.value),
        [0.0, 3.0, -170.0]
    );
    assert_eq!(
        triples[0].scalars.each_ref().map(|scalar| scalar.encoding),
        [
            crate::om::PayloadScalarEncoding::Zero,
            crate::om::PayloadScalarEncoding::Binary32,
            crate::om::PayloadScalarEncoding::Binary64,
        ]
    );
    assert_eq!(
        triples[0].scalars.each_ref().map(|scalar| scalar.offset),
        [106, 107, 111]
    );
    assert_eq!(
        triples[0]
            .scalars
            .each_ref()
            .map(|scalar| scalar.raw_value.as_slice()),
        [&bytes[6..7], &bytes[7..11], &bytes[11..19]]
    );
    assert_eq!(triples[1].body_reference_ordinal, 1);
    assert_eq!(triples[1].body_object_index, 67);
    assert_eq!(triples[1].branch, 0x11);
    assert_eq!(
        triples[1].scalars.each_ref().map(|scalar| scalar.value),
        [2.0, 0.0, 0.0]
    );
    let truncated = &bytes[..bytes.len() - 1];
    let truncated_triples = crate::om::operation_body_scalar_triples(crate::om::OperationRecord {
        bytes: truncated,
        payload: truncated,
        ..record
    });
    assert_eq!(truncated_triples.len(), 1);
    assert_eq!(truncated_triples[0], triples[0]);
}

#[test]
fn om_operation_body_branch_11_decodes_wrapped_member_lane_atomically() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "SEW",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x42\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x03\x2e\x7f\x00\x2e\x80\x01\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let members = crate::om::operation_body_members(record);
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].body_reference_ordinal, 0);
    assert_eq!(members[0].body_object_index, 66);
    assert_eq!(members[0].member_index, 127);
    assert_eq!(members[0].raw_member_index, [0x7f]);
    assert_eq!(members[0].offset, 122);
    assert_eq!(members[1].member_index, 1);
    assert_eq!(members[1].raw_member_index, [0x80, 0x01]);

    let truncated = &bytes[..bytes.len() - 1];
    assert!(
        crate::om::operation_body_members(crate::om::OperationRecord {
            bytes: truncated,
            payload: truncated,
            ..record
        })
        .is_empty()
    );
}

#[test]
fn om_trim_body_branch_11_decodes_terminal_continuation_atomically() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "TRIM BODY",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x72\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x02\x2e\x41\x00\x01\x02\x80\x43\x00\x00\x01\x72\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let continuations = crate::om::operation_body_11_continuations(record);
    assert_eq!(continuations.len(), 1);
    let continuation = &continuations[0];
    assert_eq!(continuation.body_reference_ordinal, 0);
    assert_eq!(continuation.body_object_index, 114);
    assert_eq!(continuation.continuation_index, 67);
    assert_eq!(continuation.raw_continuation_index, [0x80, 0x43]);
    assert_eq!(continuation.continuation_offset, 126);
    assert_eq!(continuation.terminal_object_index, 114);
    assert_eq!(continuation.raw_terminal_object_index, [0x72]);
    assert_eq!(continuation.terminal_offset, 131);

    let mut distinct_terminal = bytes.to_vec();
    distinct_terminal[31] = 0x71;
    assert_eq!(
        crate::om::operation_body_11_continuations(crate::om::OperationRecord {
            bytes: &distinct_terminal,
            payload: &distinct_terminal,
            ..record
        })[0]
            .terminal_object_index,
        113
    );

    let truncated = &bytes[..bytes.len() - 1];
    assert!(
        crate::om::operation_body_11_continuations(crate::om::OperationRecord {
            bytes: truncated,
            payload: truncated,
            ..record
        })
        .is_empty()
    );
}

#[test]
fn om_operation_body_decodes_homogeneous_unwrapped_reference_lanes() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "OFFSET",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let compact = b"\x01\x02\x10\x6e\xff\x1c\x00\x00\x00\x01\x03\x80\x0d\x69\x00\x00\x0b\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: compact,
        payload_offset: 100,
        payload: compact,
        label,
    };
    let lanes = crate::om::operation_body_reference_lanes(record);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].body_object_index, 110);
    assert_eq!(
        lanes[0].encoding,
        crate::om::OperationBodyReferenceLaneEncoding::CompactIndex
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| (value.object_index, value.offset))
            .collect::<Vec<_>>(),
        [(13, 111), (105, 113)]
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.raw_value.as_slice())
            .collect::<Vec<_>>(),
        [b"\x80\x0d".as_slice(), b"\x69".as_slice()]
    );

    let objects =
        b"\x01\x02\x10\x70\xff\x1c\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00";
    let object_record = crate::om::OperationRecord {
        bytes: objects,
        payload: objects,
        ..record
    };
    let lanes = crate::om::operation_body_reference_lanes(object_record);
    assert_eq!(
        lanes[0].encoding,
        crate::om::OperationBodyReferenceLaneEncoding::PayloadObjectIndex
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.object_index)
            .collect::<Vec<_>>(),
        [670, 68]
    );
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.raw_value.as_slice())
            .collect::<Vec<_>>(),
        [b"\xf1\x02\x9e".as_slice(), b"\xf0\x44".as_slice()]
    );

    let truncated = &objects[..objects.len() - 1];
    assert!(
        crate::om::operation_body_reference_lanes(crate::om::OperationRecord {
            bytes: truncated,
            payload: truncated,
            ..object_record
        })
        .is_empty()
    );

    let branch_11 =
        b"\x01\x02\x10\x70\xff\x11\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00";
    let lanes = crate::om::operation_body_reference_lanes(crate::om::OperationRecord {
        bytes: branch_11,
        payload: branch_11,
        ..record
    });
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].branch, 0x11);
    assert_eq!(
        lanes[0]
            .values
            .iter()
            .map(|value| value.object_index)
            .collect::<Vec<_>>(),
        [670, 68]
    );
}

#[test]
fn nx_extrude_construction_profile_requires_matching_resolved_encodings() {
    use crate::native::{
        FeatureExtrudeProfileReference, FeatureOperationBodyReferenceLane,
        FeatureOperationBodyReferenceLaneEncoding,
    };

    let references = [10, 11].map(|ordinal| FeatureExtrudeProfileReference {
        id: format!("profile-{ordinal}"),
        operation_label: "operation".to_string(),
        ordinal: ordinal - 10,
        witnessed: true,
        object_index: ordinal + 90,
        raw_object_index: vec![(ordinal + 90) as u8],
        data_block: Some(format!("block-{ordinal}")),
        source_offset: u64::from(ordinal),
    });
    let lane = FeatureOperationBodyReferenceLane {
        id: "lane".to_string(),
        operation_label: "operation".to_string(),
        body_reference_ordinal: 0,
        body_object_index: 42,
        branch: 0x11,
        encoding: FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex,
        object_indices: vec![100, 101],
        raw_object_indices: vec![vec![0xf0, 100], vec![0xf0, 101]],
        data_blocks: vec![Some("block-10".to_string()), Some("block-11".to_string())],
        source_offsets: vec![20, 21],
    };
    let profiles = crate::native::feature_extrude_construction_profiles(
        &references,
        std::slice::from_ref(&lane),
    );
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].body_object_index, 42);
    assert_eq!(profiles[0].object_indices, [100, 101]);
    assert_eq!(profiles[0].data_blocks, ["block-10", "block-11"]);

    for ordinal in [0, 2] {
        let mut malformed = references.clone();
        malformed[1].ordinal = ordinal;
        assert!(crate::native::feature_extrude_construction_profiles(
            &malformed,
            std::slice::from_ref(&lane),
        )
        .is_empty());
    }

    let mut mismatched = lane.clone();
    mismatched.object_indices[1] = 102;
    assert!(
        crate::native::feature_extrude_construction_profiles(&references, &[mismatched]).is_empty()
    );

    let mut unresolved = FeatureOperationBodyReferenceLane {
        id: "lane".to_string(),
        operation_label: "operation".to_string(),
        body_reference_ordinal: 0,
        body_object_index: 42,
        branch: 0x11,
        encoding: FeatureOperationBodyReferenceLaneEncoding::PayloadObjectIndex,
        object_indices: vec![100, 101],
        raw_object_indices: vec![vec![0xf0, 100], vec![0xf0, 101]],
        data_blocks: vec![Some("block-10".to_string()), Some("block-11".to_string())],
        source_offsets: vec![20, 21],
    };
    unresolved.data_blocks[1] = None;
    assert!(
        crate::native::feature_extrude_construction_profiles(&references, &[unresolved]).is_empty()
    );
}

#[test]
fn nx_operation_body_operands_require_known_distinct_body_identities() {
    use crate::native::{
        FeatureBodyReferenceOccurrence, FeatureOperationBodyMember, SegmentBodyBinding,
    };
    let member = |ordinal, member_index| FeatureOperationBodyMember {
        id: format!("nx:feature-history:operation-body-member#0-{ordinal}"),
        operation_label: "operation".to_string(),
        body_reference_ordinal: 0,
        body_object_index: 10,
        ordinal,
        member_index,
        raw_member_index: vec![member_index as u8],
        source_offset: u64::from(ordinal),
    };
    let members = [member(0, 20), member(1, 30), member(2, 10)];
    let references = [FeatureBodyReferenceOccurrence {
        id: "reference".to_string(),
        operation_label: "earlier".to_string(),
        ordinal: 0,
        body_object_index: 20,
        raw_body_object_index: vec![20],
        source_offset: 0,
    }];
    let bindings = [SegmentBodyBinding {
        id: "binding".to_string(),
        stream_link: "stream".to_string(),
        stream_ordinal: 0,
        stream_kind: "partition".to_string(),
        body_object_index: 40,
        body_alias_object_index: 30,
        stream_role: 0,
        source_offset: 0,
    }];
    let operands = crate::native::feature_operation_body_operands(&members, &references, &bindings);
    assert_eq!(
        operands
            .iter()
            .map(|operand| operand.operand_object_index)
            .collect::<Vec<_>>(),
        [20, 30]
    );
    assert!(operands[0].segment_body_bindings.is_empty());
    assert_eq!(operands[1].segment_body_bindings, ["binding"]);

    let mut second_clause = operands[0].clone();
    second_clause.body_reference_ordinal = 1;
    assert_eq!(
        operands[0].source_property_key(),
        "operation_body_operand.0.0"
    );
    assert_eq!(
        second_clause.source_property_key(),
        "operation_body_operand.1.0"
    );
}

#[test]
fn om_extrude_body_32_branch_decodes_counted_lanes() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "EXTRUDE",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let bytes = b"\x01\x02\x10\x73\xff\x32\x00\x00\x30\x77\x7e\x14\x7a\xe1\x47\xb3\x01\x03\x3d\x82\x56\x00\x3d\x82\x57\x00\x01\x04\x80\x2b\x80\x2d\x80\x2c\x01\x03\x80\x2e\x80\x77\x00\x01\x73\x00\x00";
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes,
        payload_offset: 100,
        payload: bytes,
        label,
    };
    let branch = crate::om::extrude_payload_32_branch(record).unwrap();
    assert_eq!(branch.offset, 105);
    assert_eq!(branch.body_object_index, 115);
    assert!(branch.scalar.is_finite());
    assert_eq!(branch.raw_scalar, bytes[8..16]);
    assert_eq!(branch.atoms_be, [0x3d82_5600, 0x3d82_5700]);
    assert_eq!(branch.atom_offsets, [118, 122]);
    assert_eq!(branch.atom_indices, [598, 599]);
    assert_eq!(branch.first_indices, [43, 45, 44]);
    assert_eq!(
        branch.raw_first_indices,
        [vec![0x80, 0x2b], vec![0x80, 0x2d], vec![0x80, 0x2c]]
    );
    assert_eq!(branch.first_index_offsets, [128, 130, 132]);
    assert_eq!(branch.second_indices, [46, 119]);
    assert_eq!(
        branch.raw_second_indices,
        [vec![0x80, 0x2e], vec![0x80, 0x77]]
    );
    assert_eq!(branch.second_index_offsets, [136, 138]);
    assert_eq!(branch.terminal_object_index, 115);
    assert_eq!(branch.raw_terminal_object_index, [0x73]);
    assert_eq!(branch.terminal_offset, 142);

    let mut invalid = bytes.to_vec();
    invalid[36] = 0xff;
    assert!(
        crate::om::extrude_payload_32_branch(crate::om::OperationRecord {
            bytes: &invalid,
            payload: &invalid,
            ..record
        })
        .is_none()
    );

    let mut invalid_atom = bytes.to_vec();
    invalid_atom[18] = 0x3c;
    assert!(
        crate::om::extrude_payload_32_branch(crate::om::OperationRecord {
            bytes: &invalid_atom,
            payload: &invalid_atom,
            ..record
        })
        .is_none()
    );

    let mut wrong_terminal_body = bytes.to_vec();
    wrong_terminal_body[43] = 0x72;
    assert!(
        crate::om::extrude_payload_32_branch(crate::om::OperationRecord {
            bytes: &wrong_terminal_body,
            payload: &wrong_terminal_body,
            ..record
        })
        .is_none()
    );
}

#[test]
fn nx_extrude_32_construction_requires_resolved_contiguous_profile() {
    let reference = crate::native::FeatureExtrudeProfileReference {
        id: "profile#0".to_string(),
        operation_label: "operation".to_string(),
        ordinal: 0,
        witnessed: false,
        object_index: 100,
        raw_object_index: vec![100],
        data_block: Some("block#100".to_string()),
        source_offset: 10,
    };
    let branch = crate::native::FeatureExtrudePayload32Branch {
        id: "branch".to_string(),
        operation_label: "operation".to_string(),
        body_object_index: 42,
        scalar: 1.0,
        raw_scalar: [0x2f, 0xf0, 0, 0, 0, 0, 0, 0],
        atoms_be: vec![0x3d80_0100],
        atom_source_offsets: vec![20],
        atom_indices: vec![1],
        atom_data_blocks: vec![Some("block#1".to_string())],
        first_indices: vec![2],
        raw_first_indices: vec![vec![2]],
        first_index_source_offsets: vec![21],
        first_data_blocks: vec![Some("block#2".to_string())],
        second_indices: vec![3],
        raw_second_indices: vec![vec![3]],
        second_index_source_offsets: vec![22],
        second_data_blocks: vec![Some("block#3".to_string())],
        terminal_object_index: 42,
        raw_terminal_object_index: vec![42],
        terminal_source_offset: 23,
        source_offset: 20,
    };
    let constructions = crate::native::feature_extrude_32_constructions(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&branch),
    );
    assert_eq!(constructions.len(), 1);
    assert_eq!(constructions[0].body_object_index, 42);
    assert_eq!(constructions[0].profile_references, ["profile#0"]);
    assert_eq!(constructions[0].profile_data_blocks, ["block#100"]);
    assert_eq!(constructions[0].atom_data_blocks, ["block#1"]);
    assert_eq!(constructions[0].first_data_blocks, ["block#2"]);
    assert_eq!(constructions[0].second_data_blocks, ["block#3"]);

    assert!(crate::native::feature_extrude_32_constructions(
        std::slice::from_ref(&reference),
        &[branch.clone(), branch.clone()],
    )
    .is_empty());

    let mut unresolved = reference;
    unresolved.data_block = None;
    assert!(crate::native::feature_extrude_32_constructions(
        &[unresolved],
        std::slice::from_ref(&branch),
    )
    .is_empty());
    let mut unresolved_lane = branch;
    unresolved_lane.first_data_blocks[0] = None;
    assert!(crate::native::feature_extrude_32_constructions(
        &[crate::native::FeatureExtrudeProfileReference {
            id: "profile#0".to_string(),
            operation_label: "operation".to_string(),
            ordinal: 0,
            witnessed: false,
            object_index: 100,
            raw_object_index: vec![100],
            data_block: Some("block#100".to_string()),
            source_offset: 10,
        }],
        &[unresolved_lane],
    )
    .is_empty());
}

#[test]
fn om_block_construction_field_decodes_ordered_canonical_references() {
    let label = crate::om::OperationLabel {
        header_offset: 100,
        offset: 119,
        value: "BLOCK",
        object_indices: [None; 4],
        object_index_offsets: [115, 116, 117, 118],
    };
    let mut payload = vec![0x26, 0, 0, 1, 0, 0];
    for value in 1..=18u8 {
        payload.extend([0xf0, value]);
    }
    payload.extend([0x01, 0xf1, 0x01, 0x00]);
    payload.extend([0xff; 11]);
    payload.extend([0; 4]);
    let record = crate::om::OperationRecord {
        offset: 100,
        bytes: &payload,
        payload_offset: 200,
        payload: &payload,
        label,
    };
    let field = crate::om::block_construction_references(record).unwrap();
    assert_eq!(field.control, 0x26);
    assert_eq!(field.references.len(), 19);
    assert_eq!(field.references[0].object_index, 1);
    assert_eq!(field.references[0].raw_object_index, [0xf0, 0x01]);
    assert_eq!(field.references[18].object_index, 256);
    assert_eq!(field.references[18].raw_object_index, [0xf1, 0x01, 0x00]);
    assert_eq!(field.references[0].offset, 206);

    let mut invalid = payload.clone();
    invalid[42] = 0xf0;
    assert!(
        crate::om::block_construction_references(crate::om::OperationRecord {
            bytes: &invalid,
            payload: &invalid,
            ..record
        })
        .is_none()
    );
}

#[test]
fn nx_simple_hole_construction_groups_require_shared_four_block_identity() {
    use crate::native::{
        feature_simple_hole_construction_groups, FeatureSimpleHoleRepeatedScalarLane,
        FeatureSimpleHoleRepeatedScalarLaneBlockReferences,
    };
    let lane = |operation: &str| FeatureSimpleHoleRepeatedScalarLane {
        id: format!("lane-{operation}"),
        operation_label: operation.into(),
        values: vec![25.4],
        raw_values: vec![[0x30; 8]],
        first_witness_offsets: vec![1],
        second_witness_offsets: vec![2],
    };
    let reference =
        |operation: &str, last: &str| FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
            id: format!("reference-{operation}"),
            operation_label: operation.into(),
            first_data_blocks: ["block-1".into(), "block-2".into()],
            second_data_blocks: ["block-3".into(), last.into()],
            first_reference_offsets: [3, 4],
            second_reference_offsets: [5, 6],
        };
    let lanes = [
        lane("operation#1-2"),
        lane("operation#1-3"),
        lane("operation#1-4"),
    ];
    let references = [
        reference("operation#1-4", "block-5"),
        reference("operation#1-3", "block-4"),
        reference("operation#1-2", "block-4"),
    ];
    let groups = feature_simple_hole_construction_groups(&lanes, &references);
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0].operation_labels,
        ["operation#1-2", "operation#1-3"]
    );
    assert_eq!(
        groups[0].scalar_lanes,
        ["lane-operation#1-2", "lane-operation#1-3"]
    );
    assert_eq!(
        groups[0].block_references,
        ["reference-operation#1-2", "reference-operation#1-3"]
    );

    let duplicate_references = [
        reference("operation#1-2", "block-4"),
        reference("operation#1-2", "block-4"),
    ];
    assert!(feature_simple_hole_construction_groups(&lanes, &duplicate_references).is_empty());

    let duplicate_lanes = [
        lane("operation#1-2"),
        lane("operation#1-2"),
        lane("operation#1-3"),
        lane("operation#1-4"),
    ];
    let shared_references = [
        reference("operation#1-2", "block-4"),
        reference("operation#1-3", "block-4"),
        reference("operation#1-4", "block-4"),
    ];
    assert!(
        feature_simple_hole_construction_groups(&duplicate_lanes, &shared_references).is_empty()
    );
}

#[test]
fn nx_block_construction_requires_complete_resolved_reference_field() {
    let references = (0..19)
        .map(|ordinal| crate::native::FeatureBlockConstructionReference {
            id: format!("reference#{ordinal}"),
            operation_label: "operation".to_string(),
            control: 0x26,
            ordinal,
            terminal: ordinal == 18,
            object_index: ordinal + 100,
            raw_object_index: vec![(ordinal + 100) as u8],
            data_block: Some(format!("block#{ordinal}")),
            source_offset: u64::from(ordinal),
        })
        .collect::<Vec<_>>();
    let constructions = crate::native::feature_block_constructions(&references);
    assert_eq!(constructions.len(), 1);
    assert_eq!(constructions[0].control, 0x26);
    assert_eq!(constructions[0].member_references.len(), 18);
    assert_eq!(constructions[0].terminal_reference, "reference#18");
    assert_eq!(constructions[0].terminal_data_block, "block#18");

    let mut unresolved = references;
    unresolved[7].data_block = None;
    assert!(crate::native::feature_block_constructions(&unresolved).is_empty());
}

#[test]
fn nx_block_payload_points_require_exactly_two_named_scalars() {
    use crate::native::{
        feature_block_payload_point_groups, feature_block_payload_points, FeatureBlockPayloadName,
        FeatureBlockPayloadNamedRecord, FeatureBlockPayloadScalar,
    };

    let operation_label = "operation".to_string();
    let construction_payload = "payload".to_string();
    let name = FeatureBlockPayloadName {
        id: "name".to_string(),
        operation_label: operation_label.clone(),
        construction_payload: construction_payload.clone(),
        ordinal: 0,
        type_code: Some(131),
        raw_type_code: Some(vec![0x80, 0x83]),
        type_code_payload_offset: Some(11),
        type_code_source_offset: Some(101),
        payload_leading: false,
        value: "Point7".to_string(),
        payload_offset: 10,
        source_offset: 100,
    };
    let scalar = |id: &str, ordinal: u32, value: f64| {
        let mut raw_value = value.to_be_bytes();
        raw_value[0] -= 0x10;
        FeatureBlockPayloadScalar {
            id: id.to_string(),
            operation_label: operation_label.clone(),
            construction_payload: construction_payload.clone(),
            ordinal,
            field_code: 100,
            value,
            raw_value,
            payload_offset: 20 + u64::from(ordinal) * 13,
            source_offset: 110 + u64::from(ordinal) * 13,
        }
    };
    let scalars = [scalar("first", 0, 1.25), scalar("second", 1, -2.5)];
    let record = FeatureBlockPayloadNamedRecord {
        id: "record".to_string(),
        operation_label,
        construction_payload,
        name_field: name.id.clone(),
        scalar_fields: scalars.iter().map(|scalar| scalar.id.clone()).collect(),
        payload_start_offset: 10,
        payload_end_offset: 50,
    };

    let points = feature_block_payload_points(
        std::slice::from_ref(&record),
        std::slice::from_ref(&name),
        &scalars,
    );
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].name, "Point7");
    assert_eq!(points[0].coordinates, [1.25, -2.5]);

    let mut duplicate = points[0].clone();
    duplicate.id = "point-2".to_string();
    let groups = feature_block_payload_point_groups(&[points[0].clone(), duplicate]);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].points.len(), 2);
    assert_eq!(groups[0].coordinates, [1.25, -2.5]);

    let mut conflicting = points[0].clone();
    conflicting.id = "conflicting".to_string();
    conflicting.coordinates[1] = f64::from_bits((-2.5_f64).to_bits() + 1);
    assert!(feature_block_payload_point_groups(&[points[0].clone(), conflicting]).is_empty());

    let mut incomplete = record.clone();
    incomplete.scalar_fields.pop();
    assert!(
        feature_block_payload_points(&[incomplete], std::slice::from_ref(&name), &scalars,)
            .is_empty()
    );
    let mut malformed = name;
    malformed.value = "Point0".to_string();
    assert!(feature_block_payload_points(&[record], &[malformed], &scalars).is_empty());
}

#[test]
fn om_boolean_operations_decode_counted_target_and_tools() {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x0aSUBTRACT\0\x31\x00\x00\x01\x00\x14\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\x00\x00\xe0\x7f\xff\xff\xff\x01\x01\x01\x02\x90\x19\x5e\x00\x01\x05\x90\x19\x5f\x90\x19\x44\x90\x19\x43\x90\x19\x60\x00";
    let operations = crate::om::boolean_operations(bytes, 100);
    assert_eq!(operations.len(), 1);
    assert_eq!(
        operations[0].kind,
        crate::om::BooleanOperationKind::Subtract
    );
    assert_eq!(operations[0].target, 6494);
    assert_eq!(operations[0].raw_target, [0x90, 0x19, 0x5e]);
    assert_eq!(
        operations[0].target_offset,
        100 + bytes
            .windows(3)
            .position(|window| window == [0x90, 0x19, 0x5e])
            .unwrap()
    );
    assert_eq!(operations[0].tools, [6495, 6468, 6467, 6496]);
    assert_eq!(
        operations[0].raw_tools,
        [
            vec![0x90, 0x19, 0x5f],
            vec![0x90, 0x19, 0x44],
            vec![0x90, 0x19, 0x43],
            vec![0x90, 0x19, 0x60],
        ]
    );
    assert_eq!(
        operations[0].tool_offsets,
        [0x5f, 0x44, 0x43, 0x60].map(|low| {
            100 + bytes
                .windows(3)
                .position(|window| window == [0x90, 0x19, low])
                .unwrap()
        })
    );

    let mut invalid = bytes.to_vec();
    *invalid.last_mut().unwrap() = 1;
    assert!(crate::om::boolean_operations(&invalid, 0).is_empty());
}

#[test]
fn om_index_accepts_length_framed_root_version_text() {
    let mut bytes = indexed_om_section();
    let marker = bytes
        .windows(b"\x04\x01\x0eNX 2027.3102\0".len())
        .position(|window| window == b"\x04\x01\x0eNX 2027.3102\0")
        .expect("root record");
    bytes[marker + 2] = 0x0f;
    bytes.insert(marker + 3 + 12, b' ');
    let index = bytes
        .windows(4)
        .position(|window| window == 0u32.to_le_bytes())
        .expect("index");
    for ordinal in 2..4 {
        let at = index + ordinal * 4;
        let value = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) + 1;
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let sections = crate::om::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert!(sections[0].records[0]
        .bytes
        .starts_with(b"\x04\x01\x0fNX 2027.3102 \0"));
}

#[test]
fn om_store_version_can_follow_control_prefix() {
    let bytes = b"\xff\x00prefix\x04\x01\x0eNX 2027.3102\0tail";
    let version = crate::om::store_version(bytes, 100).expect("store version");
    assert_eq!(version.offset, 108);
    assert_eq!(version.value, "NX 2027.3102");
}

#[test]
fn om_offset_only_index_bounds_storage_blocks() {
    let bytes = offset_only_indexed_om_section();
    let sections = crate::om::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].base, 0);
    assert_eq!(
        sections[0].control.as_ref().unwrap().bytes,
        &[0, 0, 0, 0, 0, 1, 0, 0]
    );
    assert_eq!(sections[0].records.len(), 2);
    assert_eq!(
        sections[0].column_storage.unwrap(),
        [sections[0].records[0].bytes, sections[0].records[1].bytes].concat()
    );
    assert_eq!(sections[0].records[0].object_id, None);
    assert!(sections[0].records[0].bytes.starts_with(b"\x04\x01\x0eNX "));
    assert_eq!(sections[0].records[1].object_id, None);
    assert!(sections[0].records[1].bytes.ends_with(b"\0"));
    let expressions = sections[0].numeric_expressions();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].name, "length");
    assert_eq!(expressions[0].value, Some(25.0));
}

#[test]
fn om_offset_only_index_accepts_one_root_record_inside_control_block() {
    let bytes = control_root_offset_only_indexed_om_section();
    let sections = crate::om::indexed_sections(&bytes);

    assert_eq!(sections.len(), 1);
    assert!(sections[0]
        .control
        .as_ref()
        .unwrap()
        .bytes
        .windows(b"NX 2027.3102".len())
        .any(|window| window == b"NX 2027.3102"));
    assert_eq!(sections[0].records.len(), 2);
    assert_eq!(sections[0].records[0].bytes, &[0; 32]);
    assert_eq!(sections[0].numeric_expressions()[0].name, "length");
}

#[test]
fn om_offset_only_index_requires_one_supported_product_record() {
    let mut duplicate = control_root_offset_only_indexed_om_section();
    let first_column = duplicate
        .windows(32)
        .position(|window| window == [0; 32])
        .expect("zero first column");
    let duplicate_product = b"\x04\x01\x0eNX 2027.3102\0";
    duplicate[first_column..first_column + duplicate_product.len()]
        .copy_from_slice(duplicate_product);
    assert!(crate::om::indexed_sections(&duplicate).is_empty());

    let mut unsupported = control_root_offset_only_indexed_om_section();
    let product = unsupported
        .windows(b"\x05\x01\x0eNX 2027.3102\0".len())
        .position(|window| window == b"\x05\x01\x0eNX 2027.3102\0")
        .expect("product record");
    unsupported[product] = 0x03;
    assert!(crate::om::indexed_sections(&unsupported).is_empty());
}

#[test]
fn om_offset_store_control_values_require_complete_zero_prefixed_words() {
    assert_eq!(
        crate::om::offset_store_control_values(&[0, 0x34, 0x12, 0, 0, 0xff, 0xff, 0xff]),
        Some(vec![0x1234, 0x00ff_ffff])
    );
    assert!(crate::om::offset_store_control_values(&[]).is_none());
    assert!(crate::om::offset_store_control_values(&[0, 1, 2]).is_none());
    assert!(crate::om::offset_store_control_values(&[1, 1, 2, 3]).is_none());
}

#[test]
fn om_offset_store_index_rows_require_complete_exact_frames() {
    let first =
        b"\x2d\x02\x0b\x2a\x93\x8a\x03\x80\x18\x20\x20\x41\x00\x47\x04\x04\x01\xc0\x44\x04\x00";
    let second = b"\x2d\x02\x0b\x83\xb6\x93\x8a\x07\x80\x18\x20\x80\x4d\x41\x00\x47\x04\x04\x01\xc0\x44\x04\x00";
    let mut bytes = b"prefix".to_vec();
    bytes.extend_from_slice(first);
    bytes.extend_from_slice(b"gap");
    bytes.extend_from_slice(second);

    let rows = crate::om::offset_store_index_rows(&bytes);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].offset, 6);
    assert_eq!(rows[0].first_index, 42);
    assert_eq!(rows[0].raw_first_index, [0x2a]);
    assert_eq!(rows[0].flag, 3);
    assert_eq!(rows[0].indices, [(24, 13), (32, 15), (32, 16), (65, 17)]);
    assert_eq!(
        rows[0].raw_indices,
        [vec![0x80, 0x18], vec![0x20], vec![0x20], vec![0x41]]
    );
    assert_eq!(rows[1].first_index, 950);
    assert_eq!(rows[1].raw_first_index, [0x83, 0xb6]);
    assert_eq!(rows[1].flag, 7);
    assert_eq!(rows[1].indices, [(24, 38), (32, 40), (77, 41), (65, 43)]);
    assert_eq!(
        rows[1].raw_indices,
        [vec![0x80, 0x18], vec![0x20], vec![0x80, 0x4d], vec![0x41]]
    );

    let mut null = first.to_vec();
    null[3] = 0xff;
    assert!(crate::om::offset_store_index_rows(&null).is_empty());
    let mut other_flag = first.to_vec();
    other_flag[6] = 0x04;
    assert!(crate::om::offset_store_index_rows(&other_flag).is_empty());
    let mut overlong = first.to_vec();
    overlong.insert(12, 0x01);
    assert!(crate::om::offset_store_index_rows(&overlong).is_empty());
    assert!(crate::om::offset_store_index_rows(&first[..first.len() - 1]).is_empty());
}

#[test]
fn om_offset_store_linked_index_rows_require_complete_exact_frames() {
    let row = b"\x02\x0b\x83\x93\x93\x8c\x16\x24\xff\xff\x90\xfe\x20\x20\x41\x00\x47\x03\x04\x01\xc0\x44\x04\x00";
    let rows = crate::om::offset_store_linked_index_rows(row);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].first_index, (915, 2));
    assert_eq!(rows[0].raw_first_index, [0x83, 0x93]);
    assert_eq!(rows[0].discriminator, 0x16);
    assert_eq!(rows[0].target_index, (36, 7));
    assert_eq!(rows[0].raw_target_index, [0x24]);
    assert_eq!(rows[0].indices, [(32, 12), (32, 13), (65, 14)]);
    assert_eq!(rows[0].raw_indices, [vec![0x20], vec![0x20], vec![0x41]]);
    assert_eq!(rows[0].flag, 3);
    assert_eq!(rows[0].mode, 4);

    let mut null = row.to_vec();
    null[7] = 0xff;
    assert!(crate::om::offset_store_linked_index_rows(&null).is_empty());
    let mut discriminator = row.to_vec();
    discriminator[6] = 0x15;
    assert!(crate::om::offset_store_linked_index_rows(&discriminator).is_empty());
    let mut flag = row.to_vec();
    flag[17] = 0x04;
    assert!(crate::om::offset_store_linked_index_rows(&flag).is_empty());
    let mut mode = row.to_vec();
    mode[18] = 0x06;
    assert!(crate::om::offset_store_linked_index_rows(&mode).is_empty());
    let mut mode_seven = row.to_vec();
    mode_seven[18] = 0x07;
    assert_eq!(
        crate::om::offset_store_linked_index_rows(&mode_seven)[0].mode,
        7
    );
    assert!(crate::om::offset_store_linked_index_rows(&row[..row.len() - 1]).is_empty());
}

#[test]
fn om_offset_store_target_index_rows_require_complete_exact_frames() {
    let row =
        b"\x02\x01\x01\x01\x16\x3e\xff\xff\x90\xfe\x1e\x20\x58\x00\x47\x03\x07\x01\xc0\x44\x04\x00";
    let rows = crate::om::offset_store_target_index_rows(row);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].target_index, (62, 5));
    assert_eq!(rows[0].raw_target_index, [0x3e]);
    assert_eq!(rows[0].indices, [(30, 10), (32, 11), (88, 12)]);
    assert_eq!(rows[0].raw_indices, [vec![0x1e], vec![0x20], vec![0x58]]);
    assert_eq!(rows[0].mode, 7);

    let mut null = row.to_vec();
    null[5] = 0xff;
    assert!(crate::om::offset_store_target_index_rows(&null).is_empty());
    let mut discriminator = row.to_vec();
    discriminator[4] = 0x17;
    assert!(crate::om::offset_store_target_index_rows(&discriminator).is_empty());
    let mut suffix = row.to_vec();
    suffix[16] = 0x03;
    assert!(crate::om::offset_store_target_index_rows(&suffix).is_empty());
    let mut mode_four = row.to_vec();
    mode_four[16] = 0x04;
    assert_eq!(
        crate::om::offset_store_target_index_rows(&mode_four)[0].mode,
        4
    );
    assert!(crate::om::offset_store_target_index_rows(&row[..row.len() - 1]).is_empty());
}

#[test]
fn om_offset_store_control_class_lane_is_a_distinct_in_range_prefix() {
    let encode = |values: &[u32]| {
        values
            .iter()
            .flat_map(|value| {
                let bytes = value.to_le_bytes();
                [0, bytes[0], bytes[1], bytes[2]]
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        crate::om::offset_store_control_class_ordinals(&encode(&[2, 0, 4, 8]), 4),
        Some(vec![2, 0])
    );
    assert!(crate::om::offset_store_control_class_ordinals(&encode(&[2, 2, 4]), 4).is_none());
    assert!(crate::om::offset_store_control_class_ordinals(&encode(&[2, 4, 1]), 4).is_none());
    assert!(crate::om::offset_store_control_class_ordinals(&encode(&[4, 8]), 4).is_none());
}

#[test]
fn om_offset_store_index_values_end_at_unique_aligned_product_record() {
    let mut bytes = vec![0, 0];
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&0x1020u32.to_le_bytes());
    bytes.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0tail");
    assert_eq!(
        crate::om::offset_store_index_values(&bytes),
        Some((2, vec![7, 0x1020]))
    );

    let mut duplicate = bytes;
    duplicate.extend_from_slice(b"\x04\x01\x0eNX 2027.3102\0");
    assert!(crate::om::offset_store_index_values(&duplicate).is_none());
    assert_eq!(
        crate::native::control_index_data_block(2, 700, 496).as_deref(),
        Some("nx:om-data-blocks-2:block#496")
    );
    assert!(crate::native::control_index_data_block(2, 700, 700).is_none());
}

#[test]
fn native_catalog_separates_offset_only_blocks_from_object_records() {
    let file =
        prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]);
    let container = container::scan_bytes(file).unwrap();

    assert!(crate::native::object_records(&container).is_empty());
    let blocks = crate::native::data_blocks(&container);
    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[0].block_ordinal, 0);
    assert_eq!(blocks[0].role, crate::native::DataBlockRole::Control);
    assert_eq!(blocks[1].role, crate::native::DataBlockRole::Column);
    assert!(blocks[0].byte_len > 0);
    let control_values = crate::native::data_block_control_values(&container);
    assert_eq!(control_values.len(), 2);
    assert_eq!(control_values[0].data_block, blocks[0].id);
    assert_eq!(control_values[0].ordinal, 0);
    assert_eq!(control_values[0].value, 0);
    assert_eq!(control_values[1].value, 1);
    let classes = crate::native::data_block_control_class_references(&container);
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].data_block, blocks[0].id);
    assert_eq!(classes[0].ordinal, 0);
    assert_eq!(classes[0].class_ordinal, 0);
    assert_eq!(classes[0].class_name, "UGS::ModlFeature");
    assert_eq!(classes[0].class_definition, "nx:om-entry-0:class#8");
    assert!(crate::native::string_values(&container).is_empty());
    assert!(crate::native::object_references(&container).is_empty());
    let expressions = crate::native::expressions(&container);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, None);
    assert_eq!(expressions[0].record, None);
}

#[test]
fn native_abr_lane_resolves_nullable_slots_within_its_offset_store() {
    let mut store = offset_only_indexed_om_section();
    let index_start = 8 + 1 + b"UGS::ModlFeature".len() + 1;
    let end_at = index_start + 3 * 4;
    let end = u32::from_le_bytes(store[end_at..end_at + 4].try_into().unwrap()) as usize;
    let mut lane = vec![0x11, 0x02];
    lane.extend_from_slice(&[0xff; 15]);
    lane.extend_from_slice(&[0x02, 0x11, b'A', b'B', b'R', 0xff, 0x03]);
    store.splice(end..end, lane.iter().copied());
    store[end_at..end_at + 4].copy_from_slice(&((end + lane.len()) as u32).to_le_bytes());
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", store)]);
    let container = container::scan_bytes(file).unwrap();

    let lanes = crate::native::data_block_abr_reference_lanes(&container);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].slot_indices[0], Some(2));
    assert_eq!(
        lanes[0].slot_data_blocks[0].as_deref(),
        Some("nx:om-data-blocks-0:block#2")
    );
    assert!(lanes[0].slot_indices[1..].iter().all(Option::is_none));
    assert_eq!(lanes[0].slot_source_offsets.len(), 16);
    assert_eq!(lanes[0].slot_source_offsets[0], lanes[0].source_offset + 1);
}

#[test]
fn om_registry_uses_length_framing_and_stays_outside_entity_payloads() {
    let mut bytes = indexed_om_section();
    bytes.extend_from_slice(b"\x10UGS::PayloadText");
    let sections = crate::om::indexed_sections(&bytes);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].types.len(), 1);
    assert_eq!(sections[0].types[0].name, "UGS::EXP_expression");
    assert_eq!(sections[0].types[0].trailing_code, 0x81);
    assert_eq!(sections[0].types[0].offset, 8);
}

#[test]
fn om_numeric_expression_retains_identity_name_unit_and_value() {
    let bytes = indexed_om_section();
    let section = crate::om::indexed_sections(&bytes).remove(0);
    let expression_records = section.numeric_expression_records();
    assert_eq!(expression_records[0].0, 1);
    let expressions = expression_records
        .iter()
        .map(|(_, expression)| expression)
        .collect::<Vec<_>>();
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, Some(0x102));
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].parameter_index, Some(8));
    assert_eq!(
        expressions[0].qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(expressions[0].unit, crate::om::ExpressionUnit::Degree);
    assert_eq!(expressions[0].expression, "120");
    assert_eq!(expressions[0].value, Some(120.0));
    let declaration = crate::om::expression_declaration_name(section.records[1].bytes).unwrap();
    assert_eq!(
        declaration.value,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(declaration.parameter_index, 8);
    assert_eq!(
        declaration.qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(declaration.literal, Some("120"));
    let declaration =
        crate::om::expression_declaration_name(b"\x04\x04p1\0\x04\x0a-5.1 * 2\0").unwrap();
    assert_eq!(declaration.value, "p1");
    assert_eq!(declaration.literal, Some("-5.1 * 2"));
    let declaration =
        crate::om::expression_declaration_name(b"\x04\x04p1\0\x04\x055.1\0\x04\x05120\0").unwrap();
    assert_eq!(declaration.literal, None);
    assert!(crate::om::expression_declaration_name(b"\x04\x04p1\0\x04\x04p2\0").is_none());
    assert!(crate::om::expression_declaration_name(b"\x04\x05p1-\0").is_none());
}

#[test]
fn om_numeric_expression_retains_formula_without_literal_value() {
    let text = b"(Number [mm]) p9: p2 * 2 + p7_radius; ";
    let mut bytes = b"hostglobalvariables".to_vec();
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);

    let expressions = crate::om::numeric_expressions(&bytes);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].name, "p9");
    assert_eq!(expressions[0].expression, "p2 * 2 + p7_radius");
    assert_eq!(expressions[0].value, None);
    assert_eq!(
        crate::native::expression_parameter_names(expressions[0].expression),
        vec!["p2", "p7_radius"]
    );
}

#[test]
fn om_numeric_expression_types_only_canonical_parameter_names() {
    for name in ["p12foo", "p12_", "p4294967296_radius"] {
        let text = format!("(Number [mm]) {name}: 5; ");
        let mut bytes = b"hostglobalvariables".to_vec();
        bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(0);

        let expressions = crate::om::numeric_expressions(&bytes);
        assert_eq!(expressions.len(), 1);
        assert_eq!(expressions[0].name, name);
        assert_eq!(expressions[0].parameter_index, None);
        assert_eq!(expressions[0].qualifier, None);
    }
    assert!(crate::om::expression_declaration_name(b"\x04\x08p12foo\0").is_none());
    assert!(crate::om::expression_declaration_name(b"\x04\x06p12_\0").is_none());
}

#[test]
fn om_numeric_expression_evaluates_constant_arithmetic_formula() {
    let text = b"(Number [mm]) p9: (193.94 - 6) / 2 + 1.5e1; ";
    let mut bytes = b"hostglobalvariables".to_vec();
    bytes.extend_from_slice(&[0x99, 0x04, (text.len() + 2) as u8]);
    bytes.extend_from_slice(text);
    bytes.push(0);

    let expressions = crate::om::numeric_expressions(&bytes);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].expression, "(193.94 - 6) / 2 + 1.5e1");
    assert_eq!(expressions[0].value, Some(108.97));
}

#[test]
fn om_numeric_expression_applies_power_before_unary_sign() {
    for (formula, expected) in [
        ("-2^2", -4.0),
        ("(-2)^2", 4.0),
        ("2^-2", 0.25),
        ("2^3^2", 512.0),
    ] {
        assert_eq!(
            crate::om::evaluate_constant_expression(formula),
            Some(expected),
            "{formula}"
        );
    }
}

#[test]
fn om_string_value_requires_marker_length_printability_and_terminator() {
    let bytes = b"\x66\x32\x03\x0cSKETCH_001\0\x66\x32\x03\x03A\0\x66\x32\x03\x03A\x01";
    let values = crate::om::string_values(bytes, 100);
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].offset, 100);
    assert_eq!(values[0].value, "SKETCH_001");
    assert_eq!(values[1].value, "A");
}

#[test]
fn om_tagged_references_preserve_family_value_order_and_bounds() {
    let bytes = b"\xe0\x12\x34\x56\x78\xca\xbc\xde\xf0\xe0\x01";
    let references = crate::om::references(bytes, 20);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].offset, 20);
    assert_eq!(
        references[0].kind,
        crate::om::ReferenceKind::PersistentHandle
    );
    assert_eq!(references[0].value, 0x1234_5678);
    assert_eq!(references[1].offset, 25);
    assert_eq!(references[1].kind, crate::om::ReferenceKind::Tagged28);
    assert_eq!(references[1].value, 0x0abc_def0);
}

#[test]
fn om_counted_record_references_require_a_complete_in_bounds_run() {
    let bytes = b"\xff\x01\x03\x90\x00\x02\x90\x00\x04\x01\x02\x90\x00\x05";
    let references = crate::om::counted_record_references(bytes, 100, 5);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].offset, 103);
    assert_eq!(
        references[0].kind,
        crate::om::ReferenceKind::RecordOrdinal16
    );
    assert_eq!(references[0].value, 2);
    assert_eq!(references[1].value, 4);
}

#[test]
fn om_record_reference_stream_requires_dense_suffix() {
    let mut dense = b"ordinary-prefix".to_vec();
    for value in 1..=8u32 {
        dense.push(0xe0);
        dense.extend_from_slice(&value.to_be_bytes());
        dense.extend_from_slice(&(0xc000_0000 | value).to_be_bytes());
    }
    let references = crate::om::dense_reference_suffix(&dense, 100);
    assert_eq!(references.len(), 16);
    assert_eq!(references[0].offset, 115);

    let mut sparse = dense;
    sparse.extend_from_slice(&[0x55; 9]);
    assert!(crate::om::dense_reference_suffix(&sparse, 0).is_empty());
}

#[test]
fn om_numeric_expression_table_is_independent_of_entity_indexing() {
    let bytes = b"hostglobalvariables\x99\x04P(Number [degrees]) p8_CircularPattern_pattern_Circular_Dir_offset_angle: 120; \x00";
    let expressions = crate::om::numeric_expressions(bytes);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, None);
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].parameter_index, Some(8));
    assert_eq!(
        expressions[0].qualifier,
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(expressions[0].value, Some(120.0));
}

/// A synthetic Parasolid partition stream: the `PS 00 00` header, a prologue with
/// a `(partition)` subtype and a schema token, then one POINT, one PLANE, one
/// CYLINDER, and one LINE record laid out back-to-back at their fixed lengths.
fn partition_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00");
    s.extend_from_slice(b"XX: TRANSMIT FILE (partition) created by modeller version 3400176\x00");
    s.extend_from_slice(b"SCH_TEST_1_9999\x00");

    // POINT (type 29): xyz at +16, metres.
    let mut pt = record(0x1d, 40);
    put_vec3(&mut pt, 16, [0.0625, 0.0, 0.0127]); // 62.5, 0, 12.7 mm
    s.extend_from_slice(&pt);

    // PLANE (type 50): origin +19, normal +43, x_axis +67.
    let mut pl = record(0x32, 91);
    pl[18] = b'+';
    put_vec3(&mut pl, 19, [0.0762, 0.0, 0.0]); // 76.2 mm
    put_vec3(&mut pl, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut pl, 67, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&pl);

    // CYLINDER (type 51): origin +19, axis +43, radius +67, x_axis +75.
    let mut cy = record(0x33, 99);
    cy[18] = b'+';
    put_vec3(&mut cy, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cy, 67, 0.004_05); // 4.05 mm
    put_vec3(&mut cy, 75, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&cy);

    // LINE (type 30): point +19, direction +43.
    let mut ln = record(0x1e, 67);
    ln[18] = b'+';
    put_vec3(&mut ln, 19, [0.01, 0.02, 0.03]);
    put_vec3(&mut ln, 43, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&ln);

    s
}

/// Raw bytes for an `ExternalReferences` container entry: an `EXTREFSTREAM`
/// index over one empty record and one four-slot handle-set record, followed by
/// an end-anchored four-string table. Decoding walks the record index, string
/// table, empty-record form, handle-set slots, and the handle/tagged tail,
/// populating every `external_reference*` arena. The record and table byte
/// shapes mirror the `external_reference_*` white-box tests.
fn external_reference_stream() -> Vec<u8> {
    let mut p = b"EXTREFSTREAM".to_vec();
    p.extend_from_slice(&[0u8; 13]); // header; byte 24 must be zero
    debug_assert_eq!(p.len(), 25);
    // Record directory (ascending offsets): empty record 7 at 45, handle-set 6 at 51.
    p.extend_from_slice(&7u32.to_le_bytes());
    p.extend_from_slice(&45u32.to_le_bytes());
    p.extend_from_slice(&6u32.to_le_bytes());
    p.extend_from_slice(&51u32.to_le_bytes());
    p.extend_from_slice(&0u32.to_le_bytes()); // terminator
    debug_assert_eq!(p.len(), 45);
    // Empty record 7: the exact six-byte form.
    p.extend_from_slice(&[1, 0, 0, 0, 0, 1]);
    debug_assert_eq!(p.len(), 51);
    // Handle-set record 6.
    p.extend_from_slice(&[1, 0, 0, 0]); // record marker
    p.extend_from_slice(&2u16.to_be_bytes()); // declared count
    p.push(1);
    for slot in [0u32, 1, 2, 3] {
        p.extend_from_slice(&slot.to_le_bytes()); // id slots
    }
    p.push(1); // record[23]
    p.push(3); // record[24] = token count
    p.extend_from_slice(&[0xe0, 0, 0, 0, 0x10]); // ascending handles
    p.extend_from_slice(&[0xe0, 0, 0, 0, 0x20]);
    p.push(3); // prefix closing count
               // Tail: one adjacent persistent-handle / tagged-reference pair.
    p.extend_from_slice(&[0xe0, 0, 0, 0, 0x05, 0xc0, 0, 0, 0x01]);
    debug_assert_eq!(p.len(), 96);
    // End-anchored string table: four strings, ordinals 0..3.
    p.push(1);
    p.extend_from_slice(&4u32.to_le_bytes());
    for value in ["child.prt", "dirA", "dirB", "extra"] {
        p.extend_from_slice(&(value.len() as u16).to_le_bytes());
        p.extend_from_slice(value.as_bytes());
    }
    p
}

/// Raw bytes for a `/Root/UG_PART/DisplayJT` container entry: a one-row outer
/// index pointing at a single embedded JT 9.4 document whose table of contents
/// declares one compressed segment. Decoding walks
/// `display_jt_indices -> display_jt_documents -> display_jt_segments ->
/// display_jt_compressed_element_sequences`, populating those arenas plus
/// `display_jt_compressed_elements`. The byte layout mirrors the
/// `display_jt_index_requires_every_declared_header` white-box test.
fn display_jt_basic_stream() -> Vec<u8> {
    let mut inflated = Vec::new();
    inflated.extend_from_slice(&24_u32.to_le_bytes());
    inflated.extend_from_slice(&[3; 16]);
    inflated.push(1);
    inflated.extend_from_slice(&5_u32.to_le_bytes());
    inflated.extend_from_slice(&[9, 8, 7]);
    inflated.extend_from_slice(&16_u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);
    inflated.extend_from_slice(&[6, 5]);
    let compressed = zlib_compress_at_level(&inflated, 1);
    let segment_byte_len = 24 + 9 + compressed.len() as u32;

    let mut data = Vec::new();
    // Outer index: version 9, one row.
    data.extend_from_slice(&9_u32.to_le_bytes());
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&100_u32.to_le_bytes()); // word-swapped value
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&28_u32.to_le_bytes()); // header offset
    data.extend_from_slice(&[0; 4]);
    // Embedded JT document header at offset 28.
    let mut version = [b' '; 80];
    version[..14].copy_from_slice(b"Version 9.4 JT");
    data.extend_from_slice(&version);
    data.push(0); // byte order
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&105_u32.to_le_bytes()); // toc offset
    data.extend_from_slice(&[1; 16]); // lsg segment id
                                      // Table of contents at offset 105: one entry.
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&[2; 16]); // segment id
    data.extend_from_slice(&137_u32.to_le_bytes()); // segment offset
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&1_u32.to_be_bytes()); // attribute (segment type 1)
                                                  // Segment at offset 137.
    data.extend_from_slice(&[2; 16]); // segment id
    data.extend_from_slice(&1_u32.to_le_bytes()); // segment type
    data.extend_from_slice(&segment_byte_len.to_le_bytes()); // header byte len
    data.extend_from_slice(&2_u32.to_le_bytes()); // compression flag
    data.extend_from_slice(&(compressed.len() as u32 + 1).to_le_bytes());
    data.push(2); // algorithm
    data.extend_from_slice(&compressed);
    data
}

/// Raw bytes for a `/Root/UG_PART/DisplayJT` container entry whose single type-7
/// shape-LOD segment frames one tri-strip LOD element. The element's base type,
/// object-type UUID, and tri-strip LOD header body decode
/// `display_jt_shape_lod_elements` and `display_jt_tri_strip_lod_headers`.
fn display_jt_shape_lod_stream() -> Vec<u8> {
    const TRI_STRIP_LOD_TYPE: [u8; 16] = [
        0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    // Tri-strip LOD header body: fixed version/binding fields then a short
    // compressed-representation tail (only hashed, not decoded, by the header).
    let mut body = Vec::new();
    body.extend_from_slice(&1u16.to_le_bytes()); // base version
    body.extend_from_slice(&1u16.to_le_bytes()); // vertex version
    body.extend_from_slice(&0u64.to_le_bytes()); // vertex bindings
    body.extend_from_slice(&1u16.to_le_bytes()); // topological mesh version
    body.extend_from_slice(&0u32.to_le_bytes()); // vertex records object id
    body.extend_from_slice(&1u16.to_le_bytes()); // compressed LOD version
    body.extend_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd]); // representation tail

    let element = jt_scene_element(TRI_STRIP_LOD_TYPE, 4, 42, &body);
    let mut payload = element;
    payload.extend_from_slice(&16u32.to_le_bytes());
    payload.extend_from_slice(&[0xff; 16]);
    payload.extend_from_slice(&[1, 0, 0, 0, 0, 0]); // segment tail

    let segment_byte_len = 24 + payload.len() as u32;
    let mut segment = Vec::new();
    segment.extend_from_slice(&[2; 16]); // segment id
    segment.extend_from_slice(&7u32.to_le_bytes()); // segment type
    segment.extend_from_slice(&segment_byte_len.to_le_bytes()); // header byte len
    segment.extend_from_slice(&payload);

    let mut data = Vec::new();
    data.extend_from_slice(&9_u32.to_le_bytes());
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&100_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&28_u32.to_le_bytes());
    data.extend_from_slice(&[0; 4]);
    let mut version = [b' '; 80];
    version[..14].copy_from_slice(b"Version 9.4 JT");
    data.extend_from_slice(&version);
    data.push(0);
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&105_u32.to_le_bytes());
    data.extend_from_slice(&[1; 16]);
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&137_u32.to_le_bytes());
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&7_u32.to_be_bytes()); // attribute type 7
    data.extend_from_slice(&segment);
    data
}

/// Raw bytes for a `/Root/UG_PART/DisplayJT` container entry whose single type-31
/// property segment inflates to one string-property atom, decoding
/// `display_jt_string_property_atoms`.
fn display_jt_string_property_stream() -> Vec<u8> {
    const STRING_PROPERTY_ATOM_TYPE: [u8; 16] = [
        0x6e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    // String atom body: the fixed prefix, a UTF-16 length, then the code units.
    let mut body = vec![1, 0, 0, 0, 0, 0x40, 1, 0];
    body.extend_from_slice(&2u32.to_le_bytes());
    for unit in "JT".encode_utf16() {
        body.extend_from_slice(&unit.to_le_bytes());
    }

    let mut inflated = jt_scene_element(STRING_PROPERTY_ATOM_TYPE, 5, 1, &body);
    inflated.extend_from_slice(&16u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);

    let compressed = zlib_compress_at_level(&inflated, 1);
    let segment_byte_len = 24 + 9 + compressed.len() as u32;

    let mut data = Vec::new();
    data.extend_from_slice(&9_u32.to_le_bytes());
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&100_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&28_u32.to_le_bytes());
    data.extend_from_slice(&[0; 4]);
    let mut version = [b' '; 80];
    version[..14].copy_from_slice(b"Version 9.4 JT");
    data.extend_from_slice(&version);
    data.push(0);
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&105_u32.to_le_bytes());
    data.extend_from_slice(&[1; 16]);
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&137_u32.to_le_bytes());
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&31_u32.to_be_bytes()); // attribute type 31
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&31_u32.to_le_bytes()); // segment type 31
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&2_u32.to_le_bytes());
    data.extend_from_slice(&(compressed.len() as u32 + 1).to_le_bytes());
    data.push(2);
    data.extend_from_slice(&compressed);
    data
}

/// Frame one JT logical element: length-prefixed `[type_id][base_type][object_id]
/// [body]`, matching `parse_jt_element_sequence`.
fn jt_scene_element(type_id: [u8; 16], base_type: u8, object_id: u32, body: &[u8]) -> Vec<u8> {
    let mut element = Vec::new();
    let byte_len = 16 + 1 + 4 + body.len();
    element.extend_from_slice(&(byte_len as u32).to_le_bytes());
    element.extend_from_slice(&type_id);
    element.push(base_type);
    element.extend_from_slice(&object_id.to_le_bytes());
    element.extend_from_slice(body);
    element
}

/// Raw bytes for a `/Root/UG_PART/DisplayJT` container entry whose single type-1
/// scene-graph segment inflates to an element sequence of one instance, group,
/// partition, range-LOD, tri-strip shape, and geometric-transform node. Each
/// node's object-type UUID, base type, and body match the corresponding
/// `display_jt9_*` white-box test, so decoding populates
/// `display_jt_base_node_data`, `_group_node_data`, `_instance_nodes`,
/// `_partition_nodes`, `_range_lod_nodes`, `_tri_strip_shape_nodes`, and
/// `_geometric_transform_attributes`.
fn display_jt_scene_graph_stream() -> Vec<u8> {
    const INSTANCE: [u8; 16] = [
        0x2a, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    const PARTITION: [u8; 16] = [
        0x3e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    const RANGE_LOD: [u8; 16] = [
        0x4c, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    const TRI_STRIP_SHAPE: [u8; 16] = [
        0x77, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    const GEOMETRIC_TRANSFORM: [u8; 16] = [
        0x83, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    // A group node whose object-type UUID matches no specialized scene node, so
    // only the base-node and group-node extractors decode it.
    const GROUP: [u8; 16] = [0x11; 16];

    // Instance node body: base header (attribute id 7) then a one-child family.
    let mut instance = Vec::new();
    instance.extend_from_slice(&1u16.to_le_bytes());
    instance.extend_from_slice(&0x20u32.to_le_bytes());
    instance.extend_from_slice(&1u32.to_le_bytes());
    instance.extend_from_slice(&7u32.to_le_bytes());
    instance.extend_from_slice(&1u16.to_le_bytes());
    instance.extend_from_slice(&9u32.to_le_bytes());

    // Group node body: base header (no attributes) then ordered children.
    let mut group = Vec::new();
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&0u32.to_le_bytes());
    group.extend_from_slice(&0u32.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&2u32.to_le_bytes());
    group.extend_from_slice(&7u32.to_le_bytes());
    group.extend_from_slice(&9u32.to_le_bytes());
    group.extend_from_slice(&[4, 3, 2, 1]);

    // Partition node body.
    let mut partition = Vec::new();
    partition.extend_from_slice(&1u16.to_le_bytes());
    partition.extend_from_slice(&0u32.to_le_bytes());
    partition.extend_from_slice(&0u32.to_le_bytes());
    partition.extend_from_slice(&1u16.to_le_bytes());
    partition.extend_from_slice(&1u32.to_le_bytes());
    partition.extend_from_slice(&2u32.to_le_bytes());
    partition.extend_from_slice(&1u32.to_le_bytes());
    partition.extend_from_slice(&1u32.to_le_bytes());
    partition.extend_from_slice(&u16::from(b'x').to_le_bytes());
    for value in [0.0f32, 1.0, 2.0, 3.0, 4.0, 5.0] {
        partition.extend_from_slice(&value.to_le_bytes());
    }
    partition.extend_from_slice(&6.0f32.to_le_bytes());
    for value in [1i32, 2, 3, 4, 5, 6] {
        partition.extend_from_slice(&value.to_le_bytes());
    }
    for value in [-3.0f32, -2.0, -1.0, 0.0, 1.0, 2.0] {
        partition.extend_from_slice(&value.to_le_bytes());
    }

    // Range-LOD node body.
    let mut range = Vec::new();
    range.extend_from_slice(&1u16.to_le_bytes());
    range.extend_from_slice(&0u32.to_le_bytes());
    range.extend_from_slice(&0u32.to_le_bytes());
    range.extend_from_slice(&1u16.to_le_bytes());
    range.extend_from_slice(&2u32.to_le_bytes());
    range.extend_from_slice(&7u32.to_le_bytes());
    range.extend_from_slice(&9u32.to_le_bytes());
    range.extend_from_slice(&1u16.to_le_bytes());
    range.extend_from_slice(&1u32.to_le_bytes());
    range.extend_from_slice(&0.25f32.to_le_bytes());
    range.extend_from_slice(&(-2i32).to_le_bytes());
    range.extend_from_slice(&1u16.to_le_bytes());
    range.extend_from_slice(&2u32.to_le_bytes());
    range.extend_from_slice(&10.0f32.to_le_bytes());
    range.extend_from_slice(&20.0f32.to_le_bytes());
    for value in [1.0f32, 2.0, 3.0] {
        range.extend_from_slice(&value.to_le_bytes());
    }

    // Tri-strip shape node body.
    let mut tri_strip = Vec::new();
    tri_strip.extend_from_slice(&1u16.to_le_bytes());
    tri_strip.extend_from_slice(&0x20u32.to_le_bytes());
    tri_strip.extend_from_slice(&0u32.to_le_bytes());
    tri_strip.extend_from_slice(&1u16.to_le_bytes());
    for value in [0.0f32, 1.0, 2.0, 3.0, 4.0, 5.0] {
        tri_strip.extend_from_slice(&value.to_le_bytes());
    }
    for value in [-3.0f32, -2.0, -1.0, 0.0, 1.0, 2.0] {
        tri_strip.extend_from_slice(&value.to_le_bytes());
    }
    tri_strip.extend_from_slice(&6.0f32.to_le_bytes());
    for value in [7i32, 8, 9, 10, 11, 12] {
        tri_strip.extend_from_slice(&value.to_le_bytes());
    }
    tri_strip.extend_from_slice(&4096u32.to_le_bytes());
    tri_strip.extend_from_slice(&0.75f32.to_le_bytes());
    tri_strip.extend_from_slice(&2u16.to_le_bytes());
    tri_strip.extend_from_slice(&0x102u64.to_le_bytes());
    tri_strip.extend_from_slice(&[24, 13, 16, 8]);
    tri_strip.extend_from_slice(&0x304u64.to_le_bytes());

    // Geometric-transform attribute body (sparse affine matrix).
    let mut geom = Vec::new();
    geom.extend_from_slice(&1u16.to_le_bytes());
    geom.push(0x08);
    geom.extend_from_slice(&0u32.to_le_bytes());
    geom.extend_from_slice(&1u16.to_le_bytes());
    geom.extend_from_slice(&0x000eu16.to_le_bytes());
    for value in [1.25f32, -2.5, 4.0] {
        geom.extend_from_slice(&value.to_le_bytes());
    }

    let mut inflated = Vec::new();
    inflated.extend_from_slice(&jt_scene_element(INSTANCE, 0, 1, &instance));
    inflated.extend_from_slice(&jt_scene_element(GROUP, 1, 2, &group));
    inflated.extend_from_slice(&jt_scene_element(PARTITION, 1, 3, &partition));
    inflated.extend_from_slice(&jt_scene_element(RANGE_LOD, 1, 4, &range));
    inflated.extend_from_slice(&jt_scene_element(TRI_STRIP_SHAPE, 2, 5, &tri_strip));
    inflated.extend_from_slice(&jt_scene_element(GEOMETRIC_TRANSFORM, 3, 6, &geom));
    // End-of-sequence marker.
    inflated.extend_from_slice(&16u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);

    let compressed = zlib_compress_at_level(&inflated, 1);
    let segment_byte_len = 24 + 9 + compressed.len() as u32;

    let mut data = Vec::new();
    data.extend_from_slice(&9_u32.to_le_bytes());
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&100_u32.to_le_bytes());
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&28_u32.to_le_bytes());
    data.extend_from_slice(&[0; 4]);
    let mut version = [b' '; 80];
    version[..14].copy_from_slice(b"Version 9.4 JT");
    data.extend_from_slice(&version);
    data.push(0);
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&105_u32.to_le_bytes());
    data.extend_from_slice(&[1; 16]);
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&137_u32.to_le_bytes());
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&1_u32.to_be_bytes());
    data.extend_from_slice(&[2; 16]);
    data.extend_from_slice(&1_u32.to_le_bytes());
    data.extend_from_slice(&segment_byte_len.to_le_bytes());
    data.extend_from_slice(&2_u32.to_le_bytes());
    data.extend_from_slice(&(compressed.len() as u32 + 1).to_le_bytes());
    data.push(2);
    data.extend_from_slice(&compressed);
    data
}

/// A Parasolid `(partition)` stream carrying the neutral-binary attribute and
/// typed-entity records (`00 4f`/`00 50` class declaration, `00 51` framed
/// entity, `00 52`/`00 53` counted value records, `00 54` string record) whose
/// extractors are exercised by the `parasolid_entity_*`, `parasolid_attribute_*`
/// white-box tests. The `00 51` entity's references resolve to the value and
/// string records, and its discriminator selects the class declaration, so the
/// join arenas (`parasolid_entity_51_numeric_uses`, `parasolid_entity_51_string_uses`,
/// `parasolid_attribute_class_uses`) are populated as well.
fn parasolid_entity_records_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00");
    s.extend_from_slice(
        b"XX: TRANSMIT FILE (partition) created by modeller\x00SCH_TEST_1_9999\x00",
    );

    // `00 4f` attribute-class declaration with identity xmt 201, followed by its
    // `00 50` field record (one field). A `00 51` entity with discriminator 200
    // resolves to this class through `definition_xmt = discriminator + 1`.
    s.extend_from_slice(&[0x00, 0x4f]);
    s.extend_from_slice(&10u32.to_be_bytes()); // name length
    s.extend_from_slice(&201u16.to_be_bytes()); // class identity xmt
    s.extend_from_slice(b"ATTR_CLASS");
    s.extend_from_slice(&[0x00, 0x50]); // field-record tag
    s.extend_from_slice(&1u32.to_be_bytes()); // field count
    s.extend_from_slice(&202u16.to_be_bytes()); // field-record xmt
    s.extend_from_slice(&0u16.to_be_bytes()); // reference 0
    s.extend_from_slice(&0u16.to_be_bytes()); // reference 1
    s.extend_from_slice(&0u16.to_be_bytes()); // header word 0
    s.extend_from_slice(&0u16.to_be_bytes()); // header word 1
    s.extend_from_slice(&[0xaa; 26]); // 26-byte descriptor prefix
    s.push(0x01); // one field code

    // `00 52` counted unsigned-integer record, identity xmt 101, one value.
    s.extend_from_slice(&[0x00, 0x52]);
    s.extend_from_slice(&1u32.to_be_bytes()); // count
    s.extend_from_slice(&101u16.to_be_bytes()); // identity xmt
    s.extend_from_slice(&7u32.to_be_bytes()); // value

    // `00 53` counted binary64 record, identity xmt 102, one finite value.
    s.extend_from_slice(&[0x00, 0x53]);
    s.extend_from_slice(&1u32.to_be_bytes()); // count
    s.extend_from_slice(&102u16.to_be_bytes()); // identity xmt
    s.extend_from_slice(&1.5f64.to_be_bytes()); // value

    // `00 54` printable string record, identity xmt 100.
    s.extend_from_slice(&[0x00, 0x54]);
    s.extend_from_slice(&10u32.to_be_bytes()); // length
    s.extend_from_slice(&100u16.to_be_bytes()); // identity xmt
    s.extend_from_slice(b"ATTR_LABEL");
    s.push(0x00); // terminator

    // `00 51` framed entity: flags 1 (low_flag 1 -> six references), identity
    // xmt 50, sequence 2, discriminator 200. Its references resolve to the
    // string (100), integer (101), and double (102) records above.
    s.extend_from_slice(&[0x00, 0x51]);
    s.extend_from_slice(&1u32.to_be_bytes()); // flags
    s.extend_from_slice(&50u16.to_be_bytes()); // identity xmt
    s.extend_from_slice(&2u32.to_be_bytes()); // sequence
    s.extend_from_slice(&200u16.to_be_bytes()); // discriminator
    for reference in [100u16, 101, 102, 150, 151, 152] {
        s.extend_from_slice(&reference.to_be_bytes());
    }
    s.extend_from_slice(&[0xaa, 0xaa]); // trailing padding

    s
}

/// A complete one-face Parasolid topology. Every ownership and geometry link is
/// a small XMT reference, so this generated fixture exercises the codec's
/// connected-B-rep path without depending on an external CAD file.
fn topology_partition_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00");
    s.extend_from_slice(
        b"XX: TRANSMIT FILE (partition) created by modeller\x00SCH_TEST_1_9999\x00",
    );

    let mut body = record(12, 24);
    put_ref(&mut body, 2, 2);
    s.extend_from_slice(&body);

    let mut shell = record(13, 24);
    put_ref(&mut shell, 2, 3);
    put_ref(&mut shell, 8, 1); // attributes
    put_ref(&mut shell, 10, 2); // body
    put_ref(&mut shell, 12, 1); // next shell
    put_ref(&mut shell, 14, 4); // first face
    put_ref(&mut shell, 16, 1); // sentinel
    put_ref(&mut shell, 18, 1); // sentinel
    put_ref(&mut shell, 20, 12); // region
    put_ref(&mut shell, 22, 1); // sentinel
    s.extend_from_slice(&shell);

    let mut face = record(14, 39);
    put_ref(&mut face, 2, 4);
    put_f64(&mut face, 10, 0.000_2); // 0.2 mm
    put_ref(&mut face, 18, 1); // next face
    put_ref(&mut face, 20, 1); // previous face
    put_ref(&mut face, 22, 5); // loop
    put_ref(&mut face, 24, 3); // shell
    put_ref(&mut face, 26, 6); // plane
    face[28] = b'+';
    s.extend_from_slice(&face);

    let mut loop_ = record(15, 16);
    put_ref(&mut loop_, 2, 5);
    put_ref(&mut loop_, 10, 7); // fin
    put_ref(&mut loop_, 12, 4); // face
    put_ref(&mut loop_, 14, 1); // next loop
    s.extend_from_slice(&loop_);

    let mut fin = record(17, 23);
    put_ref(&mut fin, 2, 7);
    put_ref(&mut fin, 6, 5); // loop
    put_ref(&mut fin, 8, 7); // next (one-fin ring)
    put_ref(&mut fin, 10, 7); // previous
    put_ref(&mut fin, 12, 10); // vertex
    put_ref(&mut fin, 14, 1); // no partner fin
    put_ref(&mut fin, 16, 8); // edge
    put_ref(&mut fin, 18, 9); // curve
    fin[22] = b'+';
    s.extend_from_slice(&fin);

    let mut edge = record(16, 32);
    put_ref(&mut edge, 2, 8);
    put_f64(&mut edge, 10, 0.000_3); // 0.3 mm
    put_ref(&mut edge, 18, 7); // fin
    put_ref(&mut edge, 24, 9); // curve
    s.extend_from_slice(&edge);

    let mut plane = record(50, 91);
    put_ref(&mut plane, 2, 6);
    plane[18] = b'+';
    put_vec3(&mut plane, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&plane);

    let mut line = record(30, 67);
    put_ref(&mut line, 2, 9);
    line[18] = b'+';
    put_vec3(&mut line, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut line, 43, [1.0, 0.0, 0.0]);
    s.extend_from_slice(&line);

    let mut vertex = record(18, 28);
    put_ref(&mut vertex, 2, 10);
    put_ref(&mut vertex, 16, 11); // point
    put_f64(&mut vertex, 18, 0.000_1); // 0.1 mm
    s.extend_from_slice(&vertex);

    let mut region = record(19, 16);
    put_ref(&mut region, 2, 12);
    s.extend_from_slice(&region);

    let mut point = record(29, 40);
    put_ref(&mut point, 2, 11);
    put_vec3(&mut point, 16, [0.01, 0.02, 0.03]);
    s.extend_from_slice(&point);
    s
}

#[test]
fn topology_retains_entity_attribute_list_references() {
    let mut stream = topology_partition_stream();
    for (kind, attribute) in [(14, 41), (15, 42), (17, 43), (16, 44), (18, 45)] {
        let at = stream
            .windows(2)
            .position(|window| window == [0, kind])
            .expect("topology record");
        put_ref(&mut stream, at + if kind == 17 { 4 } else { 8 }, attribute);
    }
    stream.extend_from_slice(&[0, 0x51]);
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&41u16.to_be_bytes());
    stream.extend_from_slice(&1u32.to_be_bytes());
    stream.extend_from_slice(&0x21u16.to_be_bytes());
    for reference in [4u16, 1, 1, 1, 1, 42] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.extend_from_slice(&[0, 0x54]);
    stream.extend_from_slice(&8u32.to_be_bytes());
    stream.extend_from_slice(&42u16.to_be_bytes());
    stream.extend_from_slice(b"deadbeef\0");

    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.get(14, 4).unwrap().face_fields().unwrap().attributes,
        41
    );
    assert_eq!(
        graph.get(15, 5).unwrap().loop_fields().unwrap().attributes,
        42
    );
    assert_eq!(
        graph.get(17, 7).unwrap().fin_fields().unwrap().attributes,
        43
    );
    assert_eq!(
        graph.get(16, 8).unwrap().edge_fields().unwrap().attributes,
        44
    );
    assert_eq!(
        graph
            .get(18, 10)
            .unwrap()
            .vertex_fields()
            .unwrap()
            .attributes,
        45
    );

    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_partition(&stream)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let references = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidTopologyAttributeListReference>(
            "parasolid_topology_attribute_list_references",
        )
        .unwrap();
    assert_eq!(references.len(), 5);
    assert_eq!(references[0].topology_type, 14);
    assert_eq!(references[0].topology_xmt, 4);
    assert_eq!(references[0].attribute_list_xmt, 41);
    assert!(references[0].attribute_list_record.is_some());
    assert_eq!(result.ir.model.attributes.len(), 1);
    assert_eq!(
        result.ir.model.attributes[0].target,
        cadmpeg_ir::attributes::AttributeTarget::Face(cadmpeg_ir::ids::FaceId(
            "nx:s0:face#4".into()
        ))
    );
    assert_eq!(
        result.ir.model.attributes[0].name,
        "parasolid_type_84_reference_5"
    );
    assert_eq!(
        result.ir.model.attributes[0].values,
        [cadmpeg_ir::attributes::AttributeValue::String(
            "deadbeef".into()
        )]
    );
}

#[test]
fn parasolid_entity_51_records_retain_layout_selected_references() {
    let mut bytes = vec![0, 0x51];
    bytes.extend_from_slice(&1u32.to_be_bytes());
    bytes.extend_from_slice(&10u16.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&0x21u16.to_be_bytes());
    for reference in 3..=8u16 {
        bytes.extend_from_slice(&reference.to_be_bytes());
    }
    bytes.extend_from_slice(&[0xaa, 0xbb]);

    let records = crate::parasolid::entity_51_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 0);
    assert_eq!(records[0].byte_len, 26);
    assert_eq!(records[0].xmt, 10);
    assert_eq!(records[0].sequence, 2);
    assert_eq!(records[0].discriminator, 0x21);
    assert_eq!(records[0].references, vec![3, 4, 5, 6, 7, 8]);
}

#[test]
fn parasolid_entity_54_strings_require_exact_length_and_terminator() {
    let mut bytes = vec![0xaa, 0x00, 0x54];
    bytes.extend_from_slice(&8u32.to_be_bytes());
    bytes.extend_from_slice(&17u16.to_be_bytes());
    bytes.extend_from_slice(b"deadbeef\0");
    bytes.extend_from_slice(&[0xbb, 0x00, 0x54, 0, 0, 0, 3, 0, 18, b'a', b'b', b'c', 1]);

    let records = crate::parasolid::entity_54_string_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].byte_len, 17);
    assert_eq!(records[0].xmt, 17);
    assert_eq!(records[0].value, "deadbeef");
}

#[test]
fn parasolid_entity_52_integers_require_complete_counted_values() {
    let mut bytes = vec![0xaa, 0x00, 0x52];
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&17u16.to_be_bytes());
    bytes.extend_from_slice(&3u32.to_be_bytes());
    bytes.extend_from_slice(&u32::MAX.to_be_bytes());

    let records = crate::parasolid::entity_52_integer_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].xmt, 17);
    assert_eq!(records[0].values, [3, u32::MAX]);
    assert_eq!(records[0].byte_len, 16);
    assert!(crate::parasolid::entity_52_integer_records(&bytes[..bytes.len() - 1]).is_empty());
}

#[test]
fn parasolid_entity_53_doubles_require_complete_finite_values() {
    let mut bytes = vec![0xaa, 0x00, 0x53, 0xff];
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&18u16.to_be_bytes());
    bytes.extend_from_slice(&0.001f64.to_be_bytes());
    bytes.extend_from_slice(&0.25f64.to_be_bytes());

    let records = crate::parasolid::entity_53_double_records(&bytes);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].offset, 1);
    assert_eq!(records[0].xmt, 18);
    assert_eq!(records[0].values, [0.001, 0.25]);
    assert_eq!(records[0].byte_len, 25);

    let last = bytes.len() - 8;
    bytes[last..].copy_from_slice(&f64::NAN.to_be_bytes());
    assert!(crate::parasolid::entity_53_double_records(&bytes).is_empty());
}

#[test]
fn topology_attribute_class_uses_resolve_instance_discriminators_by_xmt() {
    use crate::native::{
        ParasolidAttributeDefinition, ParasolidEntity51Record,
        ParasolidTopologyAttributeListReference,
    };

    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 3,
        xmt: 34,
        name: "UG2/PMARK_ATTRIBUTE".into(),
        field_count: 1,
        field_record_xmt: 19,
        field_record_references: [21, 22],
        field_record_header_words: [0, 9000],
        field_descriptor_prefix: [0; 26],
        field_storage: None,
        field_codes: vec![1],
        inflated_offset: 100,
    };
    let entity = ParasolidEntity51Record {
        id: "entity".into(),
        stream_ordinal: 3,
        xmt: 50,
        flags: 1,
        sequence: 7,
        discriminator: 0x21,
        references: vec![60, 61, 1, 62, 63, 64],
        byte_len: 26,
        inflated_offset: 200,
    };
    let reference = ParasolidTopologyAttributeListReference {
        id: "topology-reference".into(),
        stream_ordinal: 3,
        topology_type: 14,
        topology_xmt: 60,
        attribute_list_xmt: 50,
        attribute_list_record: Some(entity.id.clone()),
        inflated_offset: 300,
    };

    let instance_uses = crate::native::parasolid_attribute_class_uses(
        std::slice::from_ref(&entity),
        std::slice::from_ref(&definition),
    );
    assert_eq!(instance_uses.len(), 1);
    assert_eq!(instance_uses[0].entity_51_record, entity.id);
    assert_eq!(instance_uses[0].class_discriminator, 0x21);
    assert_eq!(instance_uses[0].definition_xmt, 34);
    assert_eq!(instance_uses[0].attribute_definition, definition.id);

    let uses = crate::native::parasolid_topology_attribute_class_uses(
        std::slice::from_ref(&reference),
        &instance_uses,
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].class_discriminator, 0x21);
    assert_eq!(uses[0].definition_xmt, 34);
    assert_eq!(uses[0].attribute_definition, definition.id);

    let mut invalid = entity;
    invalid.discriminator = 0x20;
    assert!(crate::native::parasolid_attribute_class_uses(
        std::slice::from_ref(&invalid),
        std::slice::from_ref(&definition),
    )
    .is_empty());
    assert!(crate::native::parasolid_topology_attribute_class_uses(
        &[reference],
        &crate::native::parasolid_attribute_class_uses(&[invalid], &[definition]),
    )
    .is_empty());
}

#[test]
fn topology_numeric_attribute_values_transfer_in_native_lane_order() {
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue};
    use cadmpeg_ir::ids::{FaceId, LoopId, ShellId};
    use cadmpeg_ir::AnnotationBuilder;

    use crate::native::{
        ParasolidAttributeDefinition, ParasolidEntity51NumericKind, ParasolidEntity51NumericUse,
        ParasolidEntity52IntegerRecord, ParasolidEntity53DoubleRecord,
        ParasolidTopologyAttributeClassUse, ParasolidTopologyAttributeListReference,
    };

    let mut ir = cadmpeg_ir::examples::unit_cube();
    ir.model.shells[0].id = ShellId("nx:s3:shell#58".into());
    ir.model.faces[0].id = FaceId("nx:s3:face#60".into());
    ir.model.loops[0].id = LoopId("nx:s3:loop#59".into());
    let references = [(13, 58), (14, 60), (15, 59)].map(|(topology_type, topology_xmt)| {
        ParasolidTopologyAttributeListReference {
            id: format!("topology-reference-{topology_type}"),
            stream_ordinal: 3,
            topology_type,
            topology_xmt,
            attribute_list_xmt: 50,
            attribute_list_record: Some("entity".into()),
            inflated_offset: 300,
        }
    });
    let integer = ParasolidEntity52IntegerRecord {
        id: "integers".into(),
        stream_ordinal: 3,
        xmt: 70,
        values: vec![4, u32::MAX],
        byte_len: 18,
        inflated_offset: 400,
    };
    let double = ParasolidEntity53DoubleRecord {
        id: "doubles".into(),
        stream_ordinal: 3,
        xmt: 71,
        values: vec![0.25, 7.5],
        byte_len: 26,
        inflated_offset: 500,
    };
    let uses = [
        ParasolidEntity51NumericUse {
            id: "double-use".into(),
            stream_ordinal: 3,
            entity_51_record: "entity".into(),
            reference_ordinal: 4,
            referenced_xmt: 71,
            kind: ParasolidEntity51NumericKind::Doubles,
            value_record: double.id.clone(),
            inflated_offset: 200,
        },
        ParasolidEntity51NumericUse {
            id: "integer-use".into(),
            stream_ordinal: 3,
            entity_51_record: "entity".into(),
            reference_ordinal: 3,
            referenced_xmt: 70,
            kind: ParasolidEntity51NumericKind::UnsignedIntegers,
            value_record: integer.id.clone(),
            inflated_offset: 200,
        },
    ];
    let definition = ParasolidAttributeDefinition {
        id: "definition".into(),
        stream_ordinal: 3,
        xmt: 34,
        name: "SDL/TYSA_DENSITY".into(),
        field_count: 1,
        field_record_xmt: 35,
        field_record_references: [36, 37],
        field_record_header_words: [0, 9000],
        field_descriptor_prefix: [0; 26],
        field_storage: Some(crate::native::ParasolidAttributeFieldStorage::Double),
        field_codes: vec![1],
        inflated_offset: 100,
    };
    let class_use = ParasolidTopologyAttributeClassUse {
        id: "class-use".into(),
        topology_attribute_reference: references[2].id.clone(),
        entity_51_record: "entity".into(),
        class_discriminator: 33,
        definition_xmt: definition.xmt,
        attribute_definition: definition.id.clone(),
    };
    let mut annotations = AnnotationBuilder::new();

    crate::native::attach_parasolid_topology_numeric_attributes(
        &mut ir,
        &crate::native::ParasolidNumericAttributeSources {
            topology_references: &references,
            class_uses: &[class_use],
            definitions: &[definition],
            numeric_uses: &uses,
            integers: &[integer],
            doubles: &[double],
        },
        &mut annotations,
    );

    let attributes = ir
        .model
        .attributes
        .iter()
        .filter(|attribute| attribute.id.0.contains("topology-numeric-attribute"))
        .collect::<Vec<_>>();
    assert_eq!(attributes.len(), 6);
    assert_eq!(
        attributes[0].target,
        AttributeTarget::Shell(ShellId("nx:s3:shell#58".into()))
    );
    assert_eq!(attributes[0].name, "parasolid_type_integer_reference_3");
    assert_eq!(
        attributes[4].name,
        "SDL/TYSA_DENSITY.parasolid_type_integer_reference_3"
    );
    assert_eq!(
        attributes[0].values,
        [
            AttributeValue::Integer(4),
            AttributeValue::Integer(i64::from(u32::MAX))
        ]
    );
    for (attributes, target) in [
        (
            &attributes[0..2],
            AttributeTarget::Shell(ShellId("nx:s3:shell#58".into())),
        ),
        (
            &attributes[2..4],
            AttributeTarget::Face(FaceId("nx:s3:face#60".into())),
        ),
        (
            &attributes[4..6],
            AttributeTarget::Loop(LoopId("nx:s3:loop#59".into())),
        ),
    ] {
        assert!(attributes
            .iter()
            .all(|attribute| attribute.target == target));
        assert_eq!(
            attributes[1].values,
            [AttributeValue::Float(0.25), AttributeValue::Float(7.5)]
        );
    }
}

#[test]
fn topology_rejects_shell_with_broken_face_ownership_chain() {
    let valid = topology_partition_stream();
    let graph = crate::topology::Graph::parse(&valid);
    assert_eq!(graph.body_shape_shells().len(), 1);

    let mut broken = valid;
    let face = broken
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut broken, face + 24, 99);
    assert!(crate::topology::Graph::parse(&broken)
        .body_shape_shells()
        .is_empty());

    let mut independent_previous = topology_partition_stream();
    let face = independent_previous
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut independent_previous, face + 20, 99);
    assert_eq!(
        crate::topology::Graph::parse(&independent_previous)
            .body_shape_shells()
            .len(),
        1
    );
}

#[test]
fn topology_retains_shell_body_identity_without_body_record() {
    let mut stream = topology_partition_stream();
    let body = stream
        .windows(4)
        .position(|window| window == [0, 12, 0, 2])
        .expect("body record");
    stream[body..body + 24].fill(0xff);

    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.get(12, 2).is_none());
    assert_eq!(graph.body_shape_shells().len(), 1);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.bodies[0].id.0, "nx:s0:body#2");
    assert_eq!(result.ir.model.faces.len(), 1);
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn topology_accepts_cached_last_face_and_implicit_region_identity() {
    let mut stream = topology_partition_stream();
    let shell = stream
        .windows(4)
        .position(|window| window == [0, 13, 0, 3])
        .expect("shell record");
    put_ref(&mut stream, shell + 22, 4);
    let region = stream
        .windows(4)
        .position(|window| window == [0, 19, 0, 12])
        .expect("region record");
    stream[region..region + 16].fill(0xff);
    let mut second_face = record(14, 39);
    put_ref(&mut second_face, 2, 20);
    put_f64(&mut second_face, 10, 0.000_2);
    put_ref(&mut second_face, 18, 1);
    put_ref(&mut second_face, 20, 1);
    put_ref(&mut second_face, 22, 1);
    put_ref(&mut second_face, 24, 3);
    put_ref(&mut second_face, 26, 6);
    second_face[28] = b'+';
    stream.extend(second_face);

    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.get(19, 12).is_none());
    assert_eq!(graph.body_shape_shells().len(), 1);
    assert_eq!(graph.body_shape_face_count(), 2);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.regions[0].id.0, "nx:s0:region#12");
    assert_eq!(result.ir.model.faces.len(), 2);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn topology_rejects_nonreciprocal_fin_ring() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 8, 99);
    let graph = crate::topology::Graph::parse(&stream);
    assert!(graph.face_loop_rings(4).is_none());

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.loops.is_empty());
    assert!(result.ir.model.coedges.is_empty());
    assert!(result.ir.model.edges.is_empty());

    let mut broken_partner = topology_partition_stream();
    let fin = broken_partner
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut broken_partner, fin + 14, 99);
    assert!(crate::topology::Graph::parse(&broken_partner)
        .face_loop_rings(4)
        .is_none());
}

#[test]
fn topology_accepts_fixed_record_envelope_escape() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    stream.insert(fin + 2, 0xff);
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.get(17, 7).unwrap().attribute_field_offset(),
        Some(fin + 5)
    );
    assert_eq!(graph.face_loop_rings(4).unwrap().len(), 1);
}

#[test]
fn topology_iterates_each_record_family_in_physical_order() {
    let mut stream = Vec::new();
    for (xmt, x) in [(77, 0.01), (3, 0.02)] {
        let mut point = record(29, 40);
        put_ref(&mut point, 2, xmt);
        put_vec3(&mut point, 16, [x, 0.0, 0.0]);
        stream.extend(point);
    }

    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.of_kind(29).map(|node| node.xmt).collect::<Vec<_>>(),
        vec![77, 3]
    );
}

#[test]
fn decode_synthesizes_vertex_for_closed_null_vertex_fin() {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 12, 1);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let edge = result.ir.model.edges.first().expect("closed edge");
    assert_eq!(edge.start, edge.end);
    assert!(edge.start.0.contains("closed-edge"));
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn topology_invalid_candidate_cannot_shadow_later_valid_record() {
    let mut stream = record(14, 39);
    put_ref(&mut stream, 2, 4);
    stream.extend(topology_partition_stream());

    let graph = crate::topology::Graph::parse(&stream);
    let face = graph.get(14, 4).expect("valid later FACE");
    assert!(face.pos >= 39);
    assert!(face.face_fields().is_some());
}

#[test]
fn decode_retains_topology_owned_point_at_origin() {
    let mut stream = topology_partition_stream();
    let point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, point + 16, [0.0, 0.0, 0.0]);

    assert!(crate::geometry::points(&stream).is_empty());
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph
            .get(29, 11)
            .and_then(crate::topology::Node::point_position),
        Some(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0))
    );
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.bodies[0].transform, None);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(
        result.ir.model.points[0].position,
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
    );
}

#[test]
fn decode_orders_graph_only_origin_before_later_nonzero_point() {
    let mut stream = topology_partition_stream();
    let first = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, first + 16, [0.0, 0.0, 0.0]);
    let mut second = record(29, 40);
    put_ref(&mut second, 2, 77);
    put_vec3(&mut second, 16, [0.04, 0.05, 0.06]);
    stream.extend(second);

    let graph = crate::topology::Graph::parse(&stream);
    let points = crate::decode::ordered_point_candidates(&stream, &graph);
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].0, first);
    assert_eq!(points[0].1, cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0));
    assert_eq!(points[0].2.map(|node| node.xmt), Some(11));
    assert_eq!(points[1].0, stream.len() - 40);
    assert_eq!(points[1].1, cadmpeg_ir::math::Point3::new(40.0, 50.0, 60.0));
    assert_eq!(points[1].2.map(|node| node.xmt), Some(77));
}

#[test]
fn decode_orders_graph_only_escaped_analytics_before_later_records() {
    let mut stream = topology_with_escaped_geometry_envelopes();
    let first_surface = stream
        .windows(3)
        .position(|window| window == [0, 50, 0xff])
        .expect("escaped plane record");
    let first_curve = stream
        .windows(3)
        .position(|window| window == [0, 30, 0xff])
        .expect("escaped line record");

    let second_surface_offset = stream.len();
    let mut plane = record(50, 91);
    put_ref(&mut plane, 2, 77);
    plane[18] = b'+';
    put_vec3(&mut plane, 19, [0.01, 0.02, 0.03]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    stream.extend(plane);

    let second_curve_offset = stream.len();
    let mut line = record(30, 67);
    put_ref(&mut line, 2, 78);
    line[18] = b'+';
    put_vec3(&mut line, 19, [0.04, 0.05, 0.06]);
    put_vec3(&mut line, 43, [0.0, 1.0, 0.0]);
    stream.extend(line);

    let graph = crate::topology::Graph::parse(&stream);
    let surfaces = crate::decode::ordered_surface_candidates(&stream, &graph);
    assert_eq!(surfaces.len(), 2);
    assert_eq!(surfaces[0].0, first_surface);
    assert_eq!(surfaces[0].2.map(|node| node.xmt), Some(6));
    assert_eq!(surfaces[1].0, second_surface_offset);
    assert_eq!(surfaces[1].2.map(|node| node.xmt), Some(77));

    let curves = crate::decode::ordered_curve_candidates(&stream, &graph);
    assert_eq!(curves.len(), 2);
    assert_eq!(curves[0].0, first_curve);
    assert_eq!(curves[0].2.map(|node| node.xmt), Some(9));
    assert_eq!(curves[1].0, second_curve_offset);
    assert_eq!(curves[1].2.map(|node| node.xmt), Some(78));
}

#[test]
fn decode_does_not_attach_unreferenced_point_to_solid_topology() {
    let mut stream = topology_partition_stream();
    let mut point = record(29, 40);
    put_ref(&mut point, 2, 77);
    put_vec3(&mut point, 16, [0.04, 0.05, 0.06]);
    stream.extend_from_slice(&point);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.shells[0].free_vertices.len(), 0);
    assert_eq!(result.ir.model.bodies.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_retains_connected_topology_with_unknown_surface_carrier() {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(2)
        .position(|window| window == [0, 14])
        .expect("face record");
    put_ref(&mut stream, face + 26, 99);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    let surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == result.ir.model.faces[0].surface)
        .expect("unknown face carrier");
    assert!(matches!(surface.geometry, SurfaceGeometry::Unknown { .. }));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_retains_unknown_non_null_edge_curve_carrier() {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 99);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let curve = result.ir.model.edges[0]
        .curve
        .as_ref()
        .and_then(|id| result.ir.model.curves.iter().find(|curve| &curve.id == id))
        .expect("unknown edge carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Unknown { .. }));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_drops_unknown_carrier_outside_emitted_topology() {
    let mut stream = topology_partition_stream();
    let mut orphan = record(16, 32);
    put_ref(&mut orphan, 2, 88);
    put_f64(&mut orphan, 10, 0.000_3);
    put_ref(&mut orphan, 18, 1);
    put_ref(&mut orphan, 24, 99);
    stream.extend(orphan);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert!(result
        .ir
        .model
        .curves
        .iter()
        .all(|curve| !matches!(curve.geometry, CurveGeometry::Unknown { .. })));
    assert_eq!(result.ir.model.edges.len(), 1);
}

#[test]
fn decode_retains_native_carrierless_edge() {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(2)
        .position(|window| window == [0, 16])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let fin = stream
        .windows(2)
        .position(|window| window == [0, 17])
        .expect("fin record");
    put_ref(&mut stream, fin + 18, 1);
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    let edge = &result.ir.model.edges[0];
    assert_eq!(edge.curve, None);
    assert_eq!(edge.param_range, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn tolerant_edge_becomes_a_two_support_procedural_intersection() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge_id = ir.model.edges[0].id.clone();
    ir.model.edges[0].curve = None;
    ir.model.edges[0].param_range = None;
    ir.model.edges[0].tolerance = Some(0.01);
    let mut edges = std::collections::BTreeMap::new();
    edges.insert(12, edge_id.clone());
    let graph = crate::topology::Graph::parse(&[]);
    let mut annotations = cadmpeg_ir::annotations::AnnotationBuilder::new();
    let stream = annotations.stream("nx:test");

    crate::decode::attach_tolerant_edge_intersections(
        &mut ir,
        &graph,
        &edges,
        "nx:test",
        stream,
        &mut annotations,
    );

    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id == edge_id)
        .expect("tolerant edge");
    assert_eq!(edge.param_range, Some([0.0, 1.0]));
    let curve = ir
        .model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == edge.curve.as_ref())
        .expect("procedural carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Procedural { .. }));
    let procedural = ir
        .model
        .procedural_curves
        .iter()
        .find(|procedural| procedural.curve == curve.id)
        .expect("intersection construction");
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &procedural.definition
    else {
        panic!("intersection definition");
    };
    assert!(context.sides.iter().all(|side| side.surface.is_some()));
    assert_ne!(context.sides[0].surface, context.sides[1].surface);
}

#[test]
fn intersection_support_completion_requires_one_unique_incident_complement() {
    use cadmpeg_ir::geometry::{
        IntcurveSupportContext, IntcurveSupportSide, Pcurve, ProceduralCurve,
    };
    use cadmpeg_ir::ids::{PcurveId, ProceduralCurveId};

    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge = ir.model.edges[0].clone();
    let incident = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == edge.id)
        .filter_map(|coedge| {
            let face = ir
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == coedge.owner_loop)?
                .face
                .clone();
            ir.model
                .faces
                .iter()
                .find(|candidate| candidate.id == face)
                .map(|face| face.surface.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(incident.len(), 2);
    let curve = edge.curve.expect("cube edge curve");
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("nx:test:intersection#0".into()),
        curve,
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(incident[0].clone()),
                        pcurve: None,
                    },
                    IntcurveSupportSide {
                        surface: None,
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });

    crate::decode::complete_intersection_supports_from_edge_incidence(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[1].surface.as_ref(), Some(&incident[1]));

    let pcurve_id = PcurveId("nx:test:pcurve#0".into());
    let pcurve_geometry = PcurveGeometry::Line {
        origin: Point2::new(0.0, 0.0),
        direction: Point2::new(1.0, 0.0),
    };
    ir.model.pcurves.push(Pcurve {
        id: pcurve_id.clone(),
        geometry: pcurve_geometry.clone(),
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: Some([0.0, 1.0]),
        fit_tolerance: None,
    });
    let second_face = ir
        .model
        .faces
        .iter()
        .find(|face| face.surface == incident[1])
        .expect("second incident face")
        .id
        .clone();
    let second_loop = ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.face == second_face)
        .expect("second incident loop")
        .id
        .clone();
    ir.model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.edge == edge.id && coedge.owner_loop == second_loop)
        .expect("second incident coedge")
        .pcurves = vec![cadmpeg_ir::topology::PcurveUse {
        pcurve: pcurve_id,
        isoparametric: None,
    }];

    crate::decode::complete_intersection_pcurves_from_coedge_incidence(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[1].pcurve.as_ref(), Some(&pcurve_geometry));
}

#[test]
fn opposite_intersection_chart_transfers_adaptively_within_edge_tolerance() {
    use cadmpeg_ir::geometry::{
        Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, EdgeId, ProceduralCurveId, SurfaceId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Edge;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let source = SurfaceId("synthetic:source-cylinder".into());
    let target = SurfaceId("synthetic:target-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: source.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 10.0,
            },
            source_object: None,
        },
        Surface {
            id: target.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let curve = CurveId("synthetic:intersection-curve".into());
    let construction = ProceduralCurveId("synthetic:intersection".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction: construction.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction,
        curve: curve.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(source),
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, 0.0),
                            direction: Point2::new(std::f64::consts::TAU, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(target.clone()),
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:edge".into()),
        curve: Some(curve),
        start: VertexId("synthetic:start".into()),
        end: VertexId("synthetic:end".into()),
        param_range: Some([0.0, 1.0]),
        tolerance: Some(0.01),
    });

    crate::decode::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    let pcurve = context.sides[1].pcurve.as_ref().unwrap();
    let PcurveGeometry::Nurbs { control_points, .. } = pcurve else {
        unreachable!()
    };
    assert!(control_points.len() > 2);
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let uv = cadmpeg_ir::eval::pcurve_uv(pcurve, parameter).unwrap();
        let point =
            cadmpeg_ir::eval::surface_point(&ir.model.surfaces[1].geometry, uv.u, uv.v).unwrap();
        let angle = std::f64::consts::TAU * parameter;
        assert!((point.x - 10.0 * angle.cos()).abs() < 0.01);
        assert!((point.y - 10.0 * angle.sin()).abs() < 0.01);
        assert!(point.z.abs() < 0.01);
    }
}

#[test]
fn blend_boundary_chart_uses_the_solved_curve_when_the_source_blend_is_unevaluable() {
    use cadmpeg_ir::geometry::{
        BlendSupport, Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{
        CurveId, EdgeId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::Edge;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let source = SurfaceId("synthetic:unevaluable-source-blend".into());
    let other_support = SurfaceId("synthetic:other-support".into());
    let target = SurfaceId("synthetic:target-blend".into());
    let target_construction = ProceduralSurfaceId("synthetic:target-blend-construction".into());
    ir.model.surfaces.extend([
        Surface {
            id: source.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
        Surface {
            id: other_support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: target.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: target_construction.clone(),
            },
            source_object: None,
        },
    ]);
    let spine = CurveId("synthetic:target-spine".into());
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: target_construction,
        surface: target.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface: source.clone(),
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: other_support,
                    reversed: false,
                }),
            ],
            spine: Some(spine),
            radius: BlendRadiusLaw::Constant { signed_radius: 2.0 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
    });

    let curve = CurveId("synthetic:solved-boundary".into());
    let construction = ProceduralCurveId("synthetic:boundary-intersection".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(2.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction,
        curve: curve.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(source),
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, 0.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(target),
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:boundary-edge".into()),
        curve: Some(curve),
        start: VertexId("synthetic:boundary-start".into()),
        end: VertexId("synthetic:boundary-end".into()),
        param_range: Some([0.0, 1.0]),
        tolerance: Some(1.0e-8),
    });

    crate::decode::complete_intersection_pcurves_from_opposite_charts(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    let PcurveGeometry::Nurbs { control_points, .. } = context.sides[1].pcurve.as_ref().unwrap()
    else {
        unreachable!()
    };
    assert_eq!(control_points.first(), Some(&Point2::new(0.0, 0.0)));
    assert_eq!(control_points.last(), Some(&Point2::new(1.0, 0.0)));
}

#[test]
fn tolerant_nurbs_boundary_establishes_both_intersection_charts() {
    use cadmpeg_ir::geometry::{
        Curve, IntcurveSupportContext, IntcurveSupportSide, NurbsSurface, ProceduralCurve, Surface,
    };
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, ProceduralCurveId, SurfaceId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Edge, Point, Vertex};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let nurbs = SurfaceId("synthetic:nurbs-boundary".into());
    let plane = SurfaceId("synthetic:boundary-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: nurbs.clone(),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree: 1,
                v_degree: 1,
                u_knots: vec![0.0, 0.0, 1.0, 1.0],
                v_knots: vec![0.0, 0.0, 1.0, 1.0],
                u_count: 2,
                v_count: 2,
                control_points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(0.0, 5.0, 0.0),
                    Point3::new(10.0, 0.0, 0.0),
                    Point3::new(10.0, 5.0, 0.0),
                ],
                weights: None,
                u_periodic: false,
                v_periodic: false,
            }),
            source_object: None,
        },
        Surface {
            id: plane.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let curve = CurveId("synthetic:boundary-curve".into());
    let construction = ProceduralCurveId("synthetic:boundary-intersection".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction: construction.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_curves.push(ProceduralCurve {
        id: construction,
        curve: curve.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(nurbs),
                        pcurve: None,
                    },
                    IntcurveSupportSide {
                        surface: Some(plane),
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });
    let point_ids = [
        PointId("synthetic:p0".into()),
        PointId("synthetic:p1".into()),
    ];
    let vertex_ids = [
        VertexId("synthetic:v0".into()),
        VertexId("synthetic:v1".into()),
    ];
    ir.model.points.extend([
        Point {
            id: point_ids[0].clone(),
            position: Point3::new(0.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: point_ids[1].clone(),
            position: Point3::new(10.0, 0.0, 0.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: vertex_ids[0].clone(),
            point: point_ids[0].clone(),
            tolerance: Some(1.0e-8),
        },
        Vertex {
            id: vertex_ids[1].clone(),
            point: point_ids[1].clone(),
            tolerance: Some(1.0e-8),
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:boundary-edge".into()),
        curve: Some(curve),
        start: vertex_ids[0].clone(),
        end: vertex_ids[1].clone(),
        param_range: Some([0.0, 1.0]),
        tolerance: Some(1.0e-8),
    });

    crate::decode::complete_isoparametric_intersection_pcurves(&mut ir);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    for parameter in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let points = context.sides.each_ref().map(|side| {
            let uv = cadmpeg_ir::eval::pcurve_uv(side.pcurve.as_ref().unwrap(), parameter).unwrap();
            let surface = ir
                .model
                .surfaces
                .iter()
                .find(|surface| Some(&surface.id) == side.surface.as_ref())
                .unwrap();
            cadmpeg_ir::eval::surface_point(&surface.geometry, uv.u, uv.v).unwrap()
        });
        assert!((points[0].x - 10.0 * parameter).abs() < 1.0e-8);
        assert!(
            (points[0].x - points[1].x)
                .hypot(points[0].y - points[1].y)
                .hypot(points[0].z - points[1].z)
                < 1.0e-8
        );
    }
}

#[test]
fn decode_attaches_dimension_two_bcurve_through_surface_curve() {
    let stream = pcurve_topology_partition_stream();
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.pcurves.len(), 1);
    assert_eq!(
        result.ir.model.coedges[0]
            .pcurves
            .first()
            .map(|pcurve| &pcurve.pcurve),
        Some(&result.ir.model.pcurves[0].id)
    );
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = &result.ir.model.pcurves[0].geometry
    else {
        panic!("expected NURBS pcurve");
    };
    assert_eq!(*degree, 1);
    assert_eq!(knots, &[0.0, 0.0, 1.0, 1.0]);
    assert_eq!(
        control_points,
        &[Point2::new(10.0, 20.0), Point2::new(10.0, 20.0)]
    );
    assert!(weights.is_none());
    assert!(!periodic);
    assert_eq!(result.ir.model.pcurves[0].fit_tolerance, Some(0.01));
    assert_eq!(
        result.ir.model.points[0].position,
        cadmpeg_ir::math::Point3::new(10.0, 20.0, 0.0)
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(
        validation.findings.is_empty(),
        "findings: {:?}",
        validation.findings
    );
}

#[test]
fn decode_omits_surface_curve_missing_tolerance_sentinel() {
    let mut stream = pcurve_topology_partition_stream();
    let surface_curve = stream
        .windows(2)
        .position(|window| window == [0, 137])
        .expect("surface curve");
    put_f64(
        &mut stream,
        surface_curve + 25,
        crate::decode::MISSING_TOLERANCE,
    );
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.pcurves[0].fit_tolerance, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_rejects_overflowing_pcurve_parameter_conversion() {
    let mut stream = pcurve_topology_partition_stream();
    let payload = stream
        .windows(4)
        .position(|window| window == [0, 135, 0, 22])
        .expect("pcurve payload");
    put_f64(&mut stream, payload + 15, f64::MAX);

    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.pcurves.is_empty());
    assert!(result.ir.model.coedges[0].pcurves.is_empty());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_multiple_shells_in_one_region() {
    let stream = shared_region_shells_partition_stream();
    let mut input = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec
        .decode(&mut input, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 2);
    assert_eq!(result.ir.model.regions[0].shells.len(), 2);
    assert_eq!(result.ir.model.bodies[0].regions.len(), 1);
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

fn offset_surface_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);

    let mut offset = record(60, 31);
    put_ref(&mut offset, 2, 12);
    offset[18] = b'+';
    offset[19] = b'V';
    offset[20] = 1;
    put_ref(&mut offset, 21, 6);
    put_f64(&mut offset, 23, 0.002_5);
    stream.extend(offset);
    stream
}

#[test]
fn nx_offset_surface_accepts_unbounded_representable_distance() {
    let mut stream = offset_surface_topology_partition_stream();
    let offset = stream
        .windows(4)
        .position(|window| window == [0, 60, 0, 12])
        .expect("offset record");
    put_f64(&mut stream, offset + 23, 1_001.0);
    let surfaces = crate::topology::offset_surfaces(&stream);
    let [surface] = surfaces.as_slice() else {
        panic!("offset surface")
    };
    assert_eq!(surface.distance, 1_001_000.0);

    put_f64(&mut stream, offset + 23, f64::INFINITY);
    assert!(crate::topology::offset_surfaces(&stream).is_empty());

    put_f64(&mut stream, offset + 23, f64::MAX);
    assert!(crate::topology::offset_surfaces(&stream).is_empty());
}

#[test]
fn offset_surface_envelope_does_not_consume_the_following_record() {
    let mut stream = offset_surface_topology_partition_stream();
    let offset_end = stream.len();
    let mut point = record(29, 40);
    put_ref(&mut point, 2, 20);
    put_vec3(&mut point, 16, [0.001, 0.002, 0.003]);
    stream.extend(point);

    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph.get(60, 12).map(crate::topology::Node::end),
        Some(offset_end)
    );
    assert!(graph.get(29, 20).is_some());
}

fn surface_curve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }
    let mut surface_curve = record(137, 33);
    put_ref(&mut surface_curve, 2, 12);
    surface_curve[18] = b'+';
    put_ref(&mut surface_curve, 19, 6);
    put_ref(&mut surface_curve, 21, 9);
    put_ref(&mut surface_curve, 23, 9);
    put_f64(&mut surface_curve, 25, 0.000_01);
    stream.extend(surface_curve);
    stream
}

fn pcurve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("fin record");
    put_ref(&mut stream, fin + 18, 25);
    let point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, point + 16, [0.01, 0.02, 0.0]);

    let mut wrapper = record(134, 23);
    put_ref(&mut wrapper, 2, 20);
    wrapper[18] = b'+';
    put_ref(&mut wrapper, 19, 21);
    put_ref(&mut wrapper, 21, 22);
    stream.extend(wrapper);

    let mut descriptor = record(136, 27);
    put_ref(&mut descriptor, 2, 21);
    put_ref(&mut descriptor, 4, 1);
    put_ref(&mut descriptor, 8, 2);
    put_ref(&mut descriptor, 10, 2);
    put_ref(&mut descriptor, 14, 2);
    descriptor[16] = 5;
    put_ref(&mut descriptor, 23, 23);
    put_ref(&mut descriptor, 25, 24);
    stream.extend(descriptor);

    let mut payload = record(135, 15 + 4 * 8);
    put_ref(&mut payload, 2, 22);
    payload[9..13].copy_from_slice(&4u32.to_be_bytes());
    for (index, value) in [0.01, 0.02, 0.01, 0.02].into_iter().enumerate() {
        put_f64(&mut payload, 15 + index * 8, value);
    }
    stream.extend(payload);

    let mut multiplicities = record(127, 12);
    multiplicities[4..6].copy_from_slice(&2u16.to_be_bytes());
    put_ref(&mut multiplicities, 6, 23);
    put_ref(&mut multiplicities, 8, 2);
    put_ref(&mut multiplicities, 10, 2);
    stream.extend(multiplicities);

    let mut knots = record(128, 24);
    knots[4..6].copy_from_slice(&2u16.to_be_bytes());
    put_ref(&mut knots, 6, 24);
    put_f64(&mut knots, 8, 0.0);
    put_f64(&mut knots, 16, 1.0);
    stream.extend(knots);

    let mut surface_curve = record(137, 33);
    put_ref(&mut surface_curve, 2, 25);
    surface_curve[18] = b'+';
    put_ref(&mut surface_curve, 19, 6);
    put_ref(&mut surface_curve, 21, 20);
    put_ref(&mut surface_curve, 23, 9);
    put_f64(&mut surface_curve, 25, 0.000_01);
    stream.extend(surface_curve);
    stream
}

fn shared_region_shells_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let mut shell = record(13, 24);
    put_ref(&mut shell, 2, 13);
    for (offset, reference) in [
        (8, 1),
        (10, 2),
        (12, 1),
        (14, 14),
        (16, 1),
        (18, 1),
        (20, 12),
        (22, 1),
    ] {
        put_ref(&mut shell, offset, reference);
    }
    stream.extend(shell);

    let mut face = record(14, 39);
    put_ref(&mut face, 2, 14);
    put_f64(&mut face, 10, 0.000_2);
    put_ref(&mut face, 18, 1);
    put_ref(&mut face, 20, 1);
    put_ref(&mut face, 22, 15);
    put_ref(&mut face, 24, 13);
    put_ref(&mut face, 26, 6);
    face[28] = b'+';
    stream.extend(face);

    let mut loop_ = record(15, 16);
    put_ref(&mut loop_, 2, 15);
    put_ref(&mut loop_, 10, 16);
    put_ref(&mut loop_, 12, 14);
    put_ref(&mut loop_, 14, 1);
    stream.extend(loop_);

    let mut fin = record(17, 23);
    put_ref(&mut fin, 2, 16);
    put_ref(&mut fin, 6, 15);
    put_ref(&mut fin, 8, 16);
    put_ref(&mut fin, 10, 16);
    put_ref(&mut fin, 12, 10);
    put_ref(&mut fin, 14, 1);
    put_ref(&mut fin, 16, 17);
    put_ref(&mut fin, 18, 9);
    fin[22] = b'+';
    stream.extend(fin);

    let mut edge = record(16, 32);
    put_ref(&mut edge, 2, 17);
    put_f64(&mut edge, 10, 0.000_3);
    put_ref(&mut edge, 18, 16);
    put_ref(&mut edge, 24, 9);
    stream.extend(edge);
    stream
}

fn blend_surface_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);

    let mut blend = record(56, 66);
    put_ref(&mut blend, 2, 12);
    blend[18] = b'+';
    blend[19] = b'R';
    put_ref(&mut blend, 20, 6);
    put_ref(&mut blend, 22, 6);
    put_ref(&mut blend, 24, 1);
    put_f64(&mut blend, 26, -0.003);
    put_f64(&mut blend, 34, 0.003);
    put_f64(&mut blend, 42, 1.0);
    put_f64(&mut blend, 50, 1.0);
    for at in [58, 60, 62, 64] {
        put_ref(&mut blend, at, 1);
    }
    stream.extend(blend);
    stream
}

#[test]
fn nx_blend_surface_requires_a_nonzero_rolling_ball_radius() {
    let mut stream = blend_surface_topology_partition_stream();
    let blend = stream
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("blend record");
    put_f64(&mut stream, blend + 26, 0.0);
    put_f64(&mut stream, blend + 34, 0.0);
    assert!(crate::topology::blend_surfaces(&stream).is_empty());

    put_f64(&mut stream, blend + 26, 0.5e-9);
    assert!(crate::topology::blend_surfaces(&stream).is_empty());

    put_f64(&mut stream, blend + 26, f64::MAX);
    put_f64(&mut stream, blend + 34, f64::MAX);
    assert!(crate::topology::blend_surfaces(&stream).is_empty());
}

fn blend_surface_with_extended_support_reference() -> Vec<u8> {
    let mut stream = blend_surface_topology_partition_stream();
    let blend = stream
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("blend record");
    stream.splice(blend + 20..blend + 22, [0xff, 0xfa, 0x00, 0x00]);
    stream
}

fn blend_surface_with_intersection_spine() -> Vec<u8> {
    let mut stream = blend_surface_topology_partition_stream();
    let blend = stream
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("blend record");
    put_ref(&mut stream, blend + 24, 18);

    let mut intersection = record(38, 31);
    put_ref(&mut intersection, 2, 18);
    put_ref(&mut intersection, 8, 1);
    intersection[18] = b'+';
    for (index, reference) in [6, 6, 1, 1, 1, 1].into_iter().enumerate() {
        put_ref(&mut intersection, 19 + index * 2, reference);
    }
    stream.extend(intersection);
    stream
}

fn blend_surface_with_forward_blend_support() -> Vec<u8> {
    let mut stream = blend_surface_topology_partition_stream();
    let first = stream
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("first blend record");
    put_ref(&mut stream, first + 20, 20);

    let mut second = record(56, 66);
    put_ref(&mut second, 2, 20);
    second[18] = b'+';
    second[19] = b'R';
    put_ref(&mut second, 20, 6);
    put_ref(&mut second, 22, 6);
    put_ref(&mut second, 24, 1);
    put_f64(&mut second, 26, -0.003);
    put_f64(&mut second, 34, 0.003);
    put_f64(&mut second, 42, 1.0);
    put_f64(&mut second, 50, 1.0);
    for at in [58, 60, 62, 64] {
        put_ref(&mut second, at, 1);
    }
    stream.extend(second);
    stream
}

fn intersection_curve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }
    let mut intersection = record(38, 31);
    put_ref(&mut intersection, 2, 12);
    put_ref(&mut intersection, 8, 1);
    intersection[18] = b'+';
    for (index, reference) in [6, 6, 1, 1, 1, 1].into_iter().enumerate() {
        put_ref(&mut intersection, 19 + index * 2, reference);
    }
    stream.extend(intersection);
    stream
}

fn charted_intersection_curve_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }

    let mut intersection = record(38, 31);
    put_ref(&mut intersection, 2, 12);
    put_ref(&mut intersection, 8, 1);
    intersection[18] = b'+';
    for (index, reference) in [6, 1, 20, 21, 22, 23].into_iter().enumerate() {
        put_ref(&mut intersection, 19 + index * 2, reference);
    }
    stream.extend(intersection);

    let mut chart = record(40, 108);
    chart[2..6].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut chart, 6, 20);
    put_f64(&mut chart, 8, 0.0);
    put_f64(&mut chart, 16, 1.0);
    chart[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_f64(&mut chart, 28, 0.000_01);
    put_f64(&mut chart, 36, 0.001);
    put_f64(&mut chart, 44, -31_415_800_000_000.0);
    put_f64(&mut chart, 52, -31_415_800_000_000.0);
    put_vec3(&mut chart, 60, [0.0, 0.0, 0.0]);
    put_vec3(&mut chart, 84, [0.01, 0.0, 0.0]);
    stream.extend(chart);

    for (xmt, point) in [(21, [0.0, 0.0, 0.0]), (22, [0.01, 0.0, 0.0])] {
        let mut term = record(41, 34);
        term[2..6].copy_from_slice(&1u32.to_be_bytes());
        put_ref(&mut term, 6, xmt);
        term[8..10].copy_from_slice(b"L?");
        put_vec3(&mut term, 10, point);
        stream.extend(term);
    }

    let mut uv = record(204, 41);
    uv[2..6].copy_from_slice(&4u32.to_be_bytes());
    put_ref(&mut uv, 6, 23);
    uv[8] = 2;
    for (index, value) in [0.0, 0.0, 0.01, 0.0].into_iter().enumerate() {
        put_f64(&mut uv, 9 + index * 8, value);
    }
    stream.extend(uv);
    stream
}

fn charted_intersection_with_edge_endpoint_witnesses_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let first_fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("first fin record");
    put_ref(&mut stream, first_fin + 8, 13);
    put_ref(&mut stream, first_fin + 10, 13);
    let first_point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("first point record");
    put_vec3(&mut stream, first_point + 16, [0.0, 0.0, 0.0]);

    let mut second_fin = record(17, 23);
    put_ref(&mut second_fin, 2, 13);
    put_ref(&mut second_fin, 6, 5);
    put_ref(&mut second_fin, 8, 7);
    put_ref(&mut second_fin, 10, 7);
    put_ref(&mut second_fin, 12, 14);
    put_ref(&mut second_fin, 14, 1);
    put_ref(&mut second_fin, 16, 8);
    put_ref(&mut second_fin, 18, 12);
    second_fin[22] = b'+';
    stream.extend(second_fin);

    let mut second_vertex = record(18, 28);
    put_ref(&mut second_vertex, 2, 14);
    put_ref(&mut second_vertex, 16, 15);
    put_f64(&mut second_vertex, 18, 0.000_1);
    stream.extend(second_vertex);

    let mut second_point = record(29, 40);
    put_ref(&mut second_point, 2, 15);
    put_vec3(&mut second_point, 16, [0.01, 0.0, 0.0]);
    stream.extend(second_point);
    stream
}

fn charted_intersection_without_uv_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 29, 1);
    stream
}

fn charted_intersection_with_approximated_term_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let end = stream
        .windows(8)
        .position(|window| window == [0, 41, 0, 0, 0, 1, 0, 22])
        .expect("end term record");
    put_f64(&mut stream, end + 10, 0.010_005);
    stream
}

fn ext11_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    let mut entries = vec![0u8; 2 * 11 * 8];
    for (index, point) in [[0.0, 0.0, 0.0], [0.01, 0.0, 0.0]].into_iter().enumerate() {
        let at = index * 88;
        put_vec3(&mut entries, at, point);
        put_vec3(&mut entries, at + 56, [1.0, 0.0, 0.0]);
        put_f64(&mut entries, at + 80, [2.0, 5.0][index]);
    }
    stream.splice(chart + 60..chart + 108, entries);
    stream
}

fn two_support_ext11_charted_intersection_curve_stream(ambiguous: bool) -> Vec<u8> {
    let mut stream = two_support_charted_intersection_curve_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 29, 1);

    let second_plane = stream
        .windows(4)
        .position(|window| window == [0, 50, 0, 13])
        .expect("second plane");
    if !ambiguous {
        put_vec3(&mut stream, second_plane + 67, [0.0, 0.0, 1.0]);
    }

    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    let mut entries = vec![0u8; 2 * 11 * 8];
    for (index, x) in [0.0, 0.01].into_iter().enumerate() {
        let at = index * 88;
        put_vec3(&mut entries, at, [x, 0.0, 0.0]);
        let second = if ambiguous { [x, 0.0] } else { [0.0, x] };
        put_f64(&mut entries, at + 24, x);
        put_f64(&mut entries, at + 32, second[0]);
        put_f64(&mut entries, at + 40, 0.0);
        put_f64(&mut entries, at + 48, second[1]);
        put_vec3(&mut entries, at + 56, [1.0, 0.0, 0.0]);
        put_f64(&mut entries, at + 80, x);
    }
    stream.splice(chart + 60..chart + 108, entries);
    stream
}

fn partial_ext11_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = two_support_ext11_charted_intersection_curve_stream(false);
    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    for index in 0..2 {
        put_f64(
            &mut stream,
            chart + 60 + index * 88 + 32,
            -31_415_800_000_000.0,
        );
    }
    stream
}

fn two_support_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 21, 13);

    let uv = stream
        .windows(8)
        .position(|window| window == [0, 204, 0, 0, 0, 4, 0, 23])
        .expect("UV record");
    stream[uv + 2..uv + 6].copy_from_slice(&8u32.to_be_bytes());
    stream[uv + 8] = 4;
    let mut values = vec![0u8; 8 * 8];
    for (index, value) in [0.0, 0.0, 0.0, 0.0, 0.01, 0.0, 0.01, 0.0]
        .into_iter()
        .enumerate()
    {
        put_f64(&mut values, index * 8, value);
    }
    stream.splice(uv + 9..uv + 41, values);

    let mut second_plane = record(50, 91);
    put_ref(&mut second_plane, 2, 13);
    second_plane[18] = b'+';
    put_vec3(&mut second_plane, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut second_plane, 43, [0.0, 1.0, 0.0]);
    put_vec3(&mut second_plane, 67, [1.0, 0.0, 0.0]);
    stream.extend(second_plane);
    stream
}

fn blend_bound_charted_intersection_curve_stream() -> Vec<u8> {
    let mut stream = two_support_charted_intersection_curve_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 21, 14);

    let mut bridge = record(59, 24);
    put_ref(&mut bridge, 2, 14);
    bridge[4..8].copy_from_slice(&9u32.to_be_bytes());
    for at in [8, 10, 12, 14, 16] {
        put_ref(&mut bridge, at, 1);
    }
    bridge[18] = b'+';
    put_ref(&mut bridge, 19, 0);
    put_ref(&mut bridge, 21, 13);
    stream.extend(bridge);
    stream
}

fn inline_descriptor_intersection_curve_stream() -> Vec<u8> {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let uv = stream
        .windows(8)
        .position(|window| window == [0, 204, 0, 0, 0, 4, 0, 23])
        .expect("UV record");
    let mut inline_uv = b"values\x00\x00\x00\x02\x01\x66\x01".to_vec();
    inline_uv.extend_from_slice(&4u32.to_be_bytes());
    inline_uv.extend_from_slice(&23u16.to_be_bytes());
    inline_uv.push(2);
    for value in [0.0_f64, 0.0, 0.01, 0.0] {
        inline_uv.extend_from_slice(&value.to_be_bytes());
    }
    stream.splice(uv..uv + 41, inline_uv);

    for (xmt, point) in [(22u16, [0.01_f64, 0.0, 0.0]), (21, [0.0, 0.0, 0.0])] {
        let marker = [0, 41, 0, 0, 0, 1, (xmt >> 8) as u8, xmt as u8];
        let term = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("term record");
        let mut inline = b"term_use\x00\x00\x00\x01\x01\x63\x43\x5a".to_vec();
        inline.extend_from_slice(&1u32.to_be_bytes());
        inline.extend_from_slice(&xmt.to_be_bytes());
        inline.extend_from_slice(b"L?");
        for coordinate in point {
            inline.extend_from_slice(&coordinate.to_be_bytes());
        }
        stream.splice(term..term + 34, inline);
    }
    stream
}

fn deltas_intersection_curve_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let subtype = stream
        .windows(b"(partition)".len())
        .position(|window| window == b"(partition)")
        .expect("partition subtype");
    stream.splice(
        subtype..subtype + b"(partition)".len(),
        b"(deltas)".iter().copied(),
    );
    for (tag, xmt, offset) in [(16, 8, 24), (17, 7, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_ref(&mut stream, record + offset, 12);
    }
    stream.extend_from_slice(b"intersection_data");
    stream.push(0x5a);
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&7u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream.push(b'+');
    for reference in [6u16, 6, 1, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
    }
    stream
}

/// Shared `PS`-signatured deltas-stream transmit preamble used by the deltas
/// fixture builders.
const DELTAS_PREAMBLE: &[u8] =
    b"PS\x00\x00XX: TRANSMIT FILE (deltas) created by modeller\x00SCH_TEST_1_9999\x00";

/// Append `count` deltas topology references, each the placeholder index `1`
/// followed by a set status byte, matching the deltas record framing.
fn push_reference_run(record: &mut Vec<u8>, count: usize) {
    for _ in 0..count {
        record.extend_from_slice(&1u16.to_be_bytes());
        record.push(1);
    }
}

fn status_framed_deltas_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    let mut face = Vec::new();
    face.extend_from_slice(&14u16.to_be_bytes());
    face.extend_from_slice(&100u16.to_be_bytes());
    face.extend_from_slice(&7u32.to_be_bytes());
    let push_ref = |record: &mut Vec<u8>, reference: u16| {
        record.extend_from_slice(&reference.to_be_bytes());
        record.push(1);
    };
    push_ref(&mut face, 1);
    face.extend_from_slice(&(-31_415_800_000_000.0f64).to_be_bytes());
    push_reference_run(&mut face, 5);
    face.push(b'+');
    push_reference_run(&mut face, 5);
    stream.extend(face);
    stream.extend_from_slice(&16u16.to_be_bytes());
    stream.extend_from_slice(&50_000u16.to_be_bytes());
    stream.extend_from_slice(&[0, 1]);
    stream
}

fn variable_status_framed_deltas_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&15u16.to_be_bytes());
    stream.extend_from_slice(&(-100i16).to_be_bytes());
    stream.extend_from_slice(&0u16.to_be_bytes());
    stream.extend_from_slice(&8u32.to_be_bytes());
    for reference in [1u16, 2, 3, 4] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.extend_from_slice(&17u16.to_be_bytes());
    stream.extend_from_slice(&101u16.to_be_bytes());
    stream.extend_from_slice(&9u32.to_be_bytes());
    for reference in [1u16, 2] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn status_framed_deltas_point_stream() -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(&29u16.to_be_bytes());
    stream.extend_from_slice(&50u16.to_be_bytes());
    stream.extend_from_slice(&900u32.to_be_bytes());
    for reference in [1u16; 4] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    for value in [0.0125f64, -0.002, 0.004] {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn status_framed_deltas_intersection_stream() -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(&38u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&901u32.to_be_bytes());
    for reference in [1u16, 2, 3, 4, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    for reference in [6u16, 7, 20, 21, 22, 23] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn deltas_point_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend(status_framed_deltas_point_stream());
    stream
}

fn deltas_edge_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&16u16.to_be_bytes());
    stream.extend_from_slice(&8u16.to_be_bytes());
    stream.extend_from_slice(&901u32.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream.extend_from_slice(&0.000_9f64.to_be_bytes());
    for reference in [7u16, 1, 1, 9, 1, 1, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn deltas_face_vertex_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&14u16.to_be_bytes());
    stream.extend_from_slice(&4u16.to_be_bytes());
    stream.extend_from_slice(&902u32.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream.extend_from_slice(&0.000_8f64.to_be_bytes());
    for reference in [1u16, 1, 5, 3, 6] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'+');
    push_reference_run(&mut stream, 5);

    stream.extend_from_slice(&18u16.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&903u32.to_be_bytes());
    for reference in [1u16, 1, 1, 1, 11] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.extend_from_slice(&0.000_7f64.to_be_bytes());
    stream.extend_from_slice(&1u16.to_be_bytes());
    stream.push(1);
    stream
}

fn deltas_loop_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&15u16.to_be_bytes());
    stream.extend_from_slice(&5u16.to_be_bytes());
    stream.extend_from_slice(&904u32.to_be_bytes());
    for reference in [1u16, 7, 4, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn deltas_shell_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&13u16.to_be_bytes());
    stream.extend_from_slice(&3u16.to_be_bytes());
    stream.extend_from_slice(&905u32.to_be_bytes());
    for reference in [1u16, 2, 1, 4, 1, 1, 12, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn deltas_fin_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&17u16.to_be_bytes());
    stream.extend_from_slice(&7u16.to_be_bytes());
    for reference in [1u16, 5, 7, 7, 10, 1, 8, 9, 1] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream.push(b'-');
    stream
}

/// Build a deltas analytic-surface partition record: the shared transmit
/// preamble, a `type`/`xmt`/`node_id` header, a five-reference run, the `+`
/// status marker, and the shape's big-endian `f64` payload values.
fn deltas_analytic_partition_stream(
    type_code: u16,
    xmt: u16,
    node_id: u32,
    values: &[f64],
) -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&type_code.to_be_bytes());
    stream.extend_from_slice(&xmt.to_be_bytes());
    stream.extend_from_slice(&node_id.to_be_bytes());
    push_reference_run(&mut stream, 5);
    stream.push(b'+');
    for value in values {
        stream.extend_from_slice(&value.to_be_bytes());
    }
    stream
}

fn deltas_line_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(30, 9, 906, &[0.004, 0.005, 0.006, 0.0, 1.0, 0.0])
}

fn deltas_plane_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        50,
        6,
        907,
        &[0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0],
    )
}

fn deltas_offset_surface_partition_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&60u16.to_be_bytes());
    stream.extend_from_slice(&12u16.to_be_bytes());
    stream.extend_from_slice(&907u32.to_be_bytes());
    push_reference_run(&mut stream, 5);
    stream.push(b'+');
    stream.extend_from_slice(b"V");
    stream.push(1);
    stream.extend_from_slice(&6u16.to_be_bytes());
    stream.push(1);
    stream.extend_from_slice(&0.004_5f64.to_be_bytes());
    stream
}

fn status_frame_compact_references(mut record: Vec<u8>, reference_offsets: &[usize]) -> Vec<u8> {
    for &offset in reference_offsets.iter().rev() {
        record.insert(offset + 2, 1);
    }
    record
}

fn deltas_stream_with_record(record: Vec<u8>) -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend(record);
    stream
}

fn deltas_blend_surface_partition_stream() -> Vec<u8> {
    let mut blend = record(56, 66);
    put_ref(&mut blend, 2, 12);
    blend[4..8].copy_from_slice(&908u32.to_be_bytes());
    for at in [8, 10, 12, 14, 16] {
        put_ref(&mut blend, at, 1);
    }
    blend[18] = b'+';
    blend[19] = b'R';
    put_ref(&mut blend, 20, 6);
    put_ref(&mut blend, 22, 6);
    put_ref(&mut blend, 24, 1);
    put_f64(&mut blend, 26, -0.004);
    put_f64(&mut blend, 34, 0.004);
    put_f64(&mut blend, 42, 1.0);
    put_f64(&mut blend, 50, 1.0);
    for at in [58, 60, 62, 64] {
        put_ref(&mut blend, at, 1);
    }
    deltas_stream_with_record(status_frame_compact_references(
        blend,
        &[8, 10, 12, 14, 16, 20, 22, 24, 58, 60, 62, 64],
    ))
}

fn deltas_trimmed_curve_partition_stream() -> Vec<u8> {
    let mut trim = record(133, 85);
    put_ref(&mut trim, 2, 12);
    trim[4..8].copy_from_slice(&909u32.to_be_bytes());
    for at in [8, 10, 12, 14, 16] {
        put_ref(&mut trim, at, 1);
    }
    trim[18] = b'+';
    put_ref(&mut trim, 19, 9);
    put_f64(&mut trim, 69, 0.000_3);
    put_f64(&mut trim, 77, 0.000_7);
    deltas_stream_with_record(status_frame_compact_references(
        trim,
        &[8, 10, 12, 14, 16, 19],
    ))
}

fn deltas_surface_curve_partition_stream() -> Vec<u8> {
    let mut surface_curve = record(137, 33);
    put_ref(&mut surface_curve, 2, 12);
    surface_curve[4..8].copy_from_slice(&910u32.to_be_bytes());
    for at in [8, 10, 12, 14, 16] {
        put_ref(&mut surface_curve, at, 1);
    }
    surface_curve[18] = b'+';
    put_ref(&mut surface_curve, 19, 6);
    put_ref(&mut surface_curve, 21, 9);
    put_ref(&mut surface_curve, 23, 9);
    put_f64(&mut surface_curve, 25, 0.000_02);
    deltas_stream_with_record(status_frame_compact_references(
        surface_curve,
        &[8, 10, 12, 14, 16, 19, 21, 23],
    ))
}

/// Point the single partition face record at geometry reference `reference`.
fn link_partition_face(stream: &mut Vec<u8>, reference: u16) {
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face record");
    put_ref(stream, face + 26, reference);
}

/// Point both the edge and fin topology records at geometry reference
/// `reference`.
fn link_partition_edge_and_fin(stream: &mut Vec<u8>, reference: u16) {
    for (kind, xmt, field) in [(16u8, 8u8, 24usize), (17, 7, 18)] {
        let record = stream
            .windows(4)
            .position(|window| window == [0, kind, 0, xmt])
            .expect("topology record");
        put_ref(stream, record + field, reference);
    }
}

fn circle_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_edge_and_fin(&mut stream, 12);
    let mut circle = record(31, 99);
    put_ref(&mut circle, 2, 12);
    circle[18] = b'+';
    put_vec3(&mut circle, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut circle, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut circle, 67, [1.0, 0.0, 0.0]);
    put_f64(&mut circle, 91, 0.01);
    stream.extend(circle);
    stream
}

fn deltas_circle_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        31,
        12,
        908,
        &[0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.025],
    )
}

fn ellipse_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_edge_and_fin(&mut stream, 13);
    let mut ellipse = record(32, 107);
    put_ref(&mut ellipse, 2, 13);
    ellipse[18] = b'+';
    put_vec3(&mut ellipse, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut ellipse, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut ellipse, 67, [1.0, 0.0, 0.0]);
    put_f64(&mut ellipse, 91, 0.02);
    put_f64(&mut ellipse, 99, 0.01);
    stream.extend(ellipse);
    stream
}

fn deltas_ellipse_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        32,
        13,
        909,
        &[
            0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.03, 0.012,
        ],
    )
}

fn cylinder_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);
    let mut cylinder = record(51, 99);
    put_ref(&mut cylinder, 2, 12);
    cylinder[18] = b'+';
    put_vec3(&mut cylinder, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cylinder, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cylinder, 67, 0.01);
    put_vec3(&mut cylinder, 75, [1.0, 0.0, 0.0]);
    stream.extend(cylinder);
    stream
}

fn deltas_cylinder_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        51,
        12,
        910,
        &[0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 0.025, 1.0, 0.0, 0.0],
    )
}

fn cone_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);
    let mut cone = record(52, 115);
    put_ref(&mut cone, 2, 12);
    cone[18] = b'+';
    put_vec3(&mut cone, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cone, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cone, 67, 0.01);
    put_f64(&mut cone, 75, 0.0);
    put_f64(&mut cone, 83, 1.0);
    put_vec3(&mut cone, 91, [1.0, 0.0, 0.0]);
    stream.extend(cone);
    stream
}

fn deltas_cone_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        52,
        12,
        911,
        &[
            0.001,
            0.002,
            0.003,
            0.0,
            1.0,
            0.0,
            0.025,
            0.5,
            3.0f64.sqrt() / 2.0,
            1.0,
            0.0,
            0.0,
        ],
    )
}

fn sphere_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);
    let mut sphere = record(53, 99);
    put_ref(&mut sphere, 2, 12);
    sphere[18] = b'+';
    put_vec3(&mut sphere, 19, [0.0, 0.0, 0.0]);
    put_f64(&mut sphere, 43, 0.01);
    put_vec3(&mut sphere, 51, [0.0, 0.0, 1.0]);
    put_vec3(&mut sphere, 75, [1.0, 0.0, 0.0]);
    stream.extend(sphere);
    stream
}

fn deltas_sphere_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        53,
        12,
        912,
        &[0.001, 0.002, 0.003, 0.025, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0],
    )
}

fn torus_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    link_partition_face(&mut stream, 12);
    let mut torus = record(54, 107);
    put_ref(&mut torus, 2, 12);
    torus[18] = b'+';
    put_vec3(&mut torus, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut torus, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut torus, 67, 0.03);
    put_f64(&mut torus, 75, 0.01);
    put_vec3(&mut torus, 83, [1.0, 0.0, 0.0]);
    stream.extend(torus);
    stream
}

fn deltas_torus_partition_stream() -> Vec<u8> {
    deltas_analytic_partition_stream(
        54,
        12,
        913,
        &[
            0.001, 0.002, 0.003, 0.0, 1.0, 0.0, 0.04, 0.015, 1.0, 0.0, 0.0,
        ],
    )
}

fn bspline_partition_stream() -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"PS\x00\x00XX: TRANSMIT FILE (partition)\x00SCH_TEST_1_9999\x00");
    let mut surface = record(124, 23);
    put_ref(&mut surface, 2, 10);
    surface[18] = b'+';
    put_ref(&mut surface, 19, 20);
    put_ref(&mut surface, 21, 21);
    s.extend(surface);

    let mut descriptor = record(126, 48);
    put_ref(&mut descriptor, 2, 20);
    put_ref(&mut descriptor, 6, 1);
    put_ref(&mut descriptor, 8, 1);
    put_ref(&mut descriptor, 12, 2);
    put_ref(&mut descriptor, 16, 2);
    descriptor[18] = 5;
    descriptor[19] = 5;
    descriptor[20..24].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut descriptor, 36, 30);
    put_ref(&mut descriptor, 38, 31);
    put_ref(&mut descriptor, 40, 32);
    put_ref(&mut descriptor, 42, 33);
    put_ref(&mut descriptor, 44, 125);
    put_ref(&mut descriptor, 46, 21);
    s.extend(descriptor);

    let mut data = record(125, 97 + 12 * 8);
    put_ref(&mut data, 2, 21);
    data[90] = b'+';
    data[91..95].copy_from_slice(&12u32.to_be_bytes());
    for (index, value) in [
        0.0, 0.0, 0.0, 0.0, 0.02, 0.0, 0.01, 0.0, 0.0, 0.01, 0.02, 0.0,
    ]
    .into_iter()
    .enumerate()
    {
        put_f64(&mut data, 97 + index * 8, value);
    }
    s.extend(data);

    for (tag, reference, values) in [(127, 30, vec![2u16, 2]), (127, 31, vec![2, 2])] {
        let mut array = record(tag, 8 + values.len() * 2);
        array[4..6].copy_from_slice(&(values.len() as u16).to_be_bytes());
        put_ref(&mut array, 6, reference);
        for (index, value) in values.into_iter().enumerate() {
            put_ref(&mut array, 8 + index * 2, value);
        }
        s.extend(array);
    }
    for reference in [32, 33] {
        let mut array = record(128, 8 + 2 * 8);
        array[4..6].copy_from_slice(&2u16.to_be_bytes());
        put_ref(&mut array, 6, reference);
        put_f64(&mut array, 8, 0.0);
        put_f64(&mut array, 16, 1.0);
        s.extend(array);
    }

    let mut curve = record(134, 23);
    put_ref(&mut curve, 2, 50);
    curve[18] = b'+';
    put_ref(&mut curve, 19, 40);
    put_ref(&mut curve, 21, 41);
    s.extend(curve);
    let mut curve_descriptor = record(136, 27);
    put_ref(&mut curve_descriptor, 2, 40);
    put_ref(&mut curve_descriptor, 4, 1);
    put_ref(&mut curve_descriptor, 8, 2);
    put_ref(&mut curve_descriptor, 10, 3);
    put_ref(&mut curve_descriptor, 14, 2);
    curve_descriptor[16] = 5;
    put_ref(&mut curve_descriptor, 23, 42);
    put_ref(&mut curve_descriptor, 25, 43);
    s.extend(curve_descriptor);
    let mut curve_data = record(135, 15 + 6 * 8);
    put_ref(&mut curve_data, 2, 41);
    curve_data[9..13].copy_from_slice(&6u32.to_be_bytes());
    for (index, value) in [0.0, 0.0, 0.0, 0.02, 0.0, 0.0].into_iter().enumerate() {
        put_f64(&mut curve_data, 15 + index * 8, value);
    }
    s.extend(curve_data);
    for (tag, reference) in [(127, 42), (128, 43)] {
        let mut array = record(tag, if tag == 127 { 12 } else { 24 });
        array[4..6].copy_from_slice(&2u16.to_be_bytes());
        put_ref(&mut array, 6, reference);
        if tag == 127 {
            put_ref(&mut array, 8, 2);
            put_ref(&mut array, 10, 2);
        } else {
            put_f64(&mut array, 8, 0.0);
            put_f64(&mut array, 16, 1.0);
        }
        s.extend(array);
    }
    s
}

fn extended_bspline_surface_stream() -> Vec<u8> {
    let descriptor_ref = 40_000u32;
    let payload_ref = 40_001u32;
    let support_refs = [40_010u32, 40_011, 40_012, 40_013];

    let mut stream = Vec::new();
    let mut wrapper = record(124, 19);
    put_ref(&mut wrapper, 2, 10);
    wrapper[18] = b'+';
    stream.extend(wrapper);
    stream.extend(encoded_xmt(descriptor_ref));
    stream.extend(encoded_xmt(payload_ref));

    let xmt = encoded_xmt(descriptor_ref);
    let shift = xmt.len() - 2;
    let mut descriptor = vec![0u8; 58 + shift];
    descriptor[..2].copy_from_slice(&126u16.to_be_bytes());
    descriptor[2..2 + xmt.len()].copy_from_slice(&xmt);
    put_ref(&mut descriptor, 6 + shift, 1);
    put_ref(&mut descriptor, 8 + shift, 1);
    put_ref(&mut descriptor, 12 + shift, 2);
    put_ref(&mut descriptor, 16 + shift, 2);
    descriptor[18 + shift] = 5;
    descriptor[19 + shift] = 5;
    descriptor[20 + shift..24 + shift].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24 + shift..28 + shift].copy_from_slice(&2u32.to_be_bytes());
    let mut at = 34 + shift;
    for reference in [
        40_009,
        support_refs[0],
        support_refs[1],
        support_refs[2],
        support_refs[3],
    ] {
        let encoded = encoded_xmt(reference);
        descriptor[at..at + encoded.len()].copy_from_slice(&encoded);
        at += encoded.len();
    }
    assert_eq!(at, 54 + shift);
    put_ref(&mut descriptor, 54 + shift, 125);
    stream.extend(descriptor);

    let xmt = encoded_xmt(payload_ref);
    let shift = xmt.len() - 2;
    let first = encoded_xmt(40_020);
    let data_at = 95 + shift + first.len();
    let mut payload = vec![0u8; data_at + 12 * 8];
    payload[..2].copy_from_slice(&125u16.to_be_bytes());
    payload[2..2 + xmt.len()].copy_from_slice(&xmt);
    payload[90 + shift] = b'+';
    payload[91 + shift..95 + shift].copy_from_slice(&12u32.to_be_bytes());
    payload[95 + shift..data_at].copy_from_slice(&first);
    for (index, value) in [
        0.0, 0.0, 0.0, 0.0, 0.02, 0.0, 0.01, 0.0, 0.0, 0.01, 0.02, 0.0,
    ]
    .into_iter()
    .enumerate()
    {
        put_f64(&mut payload, data_at + index * 8, value);
    }
    stream.extend(payload);

    for (tag, reference, values) in [
        (127, support_refs[0], vec![2u16, 2]),
        (127, support_refs[1], vec![2, 2]),
    ] {
        let reference = encoded_xmt(reference);
        let mut array = record(tag, 6 + reference.len() + values.len() * 2);
        array[4..6].copy_from_slice(&(values.len() as u16).to_be_bytes());
        array[6..6 + reference.len()].copy_from_slice(&reference);
        for (index, value) in values.into_iter().enumerate() {
            put_ref(&mut array, 6 + reference.len() + index * 2, value);
        }
        stream.extend(array);
    }
    for reference in [support_refs[2], support_refs[3]] {
        let reference = encoded_xmt(reference);
        let mut array = record(128, 6 + reference.len() + 16);
        array[4..6].copy_from_slice(&2u16.to_be_bytes());
        array[6..6 + reference.len()].copy_from_slice(&reference);
        put_f64(&mut array, 6 + reference.len(), 0.0);
        put_f64(&mut array, 14 + reference.len(), 1.0);
        stream.extend(array);
    }
    stream
}

fn bspline_surface_replacement_partition_stream() -> Vec<u8> {
    let mut stream = bspline_partition_stream();
    let mut descriptor = record(126, 48);
    put_ref(&mut descriptor, 2, 60);
    put_ref(&mut descriptor, 6, 1);
    put_ref(&mut descriptor, 8, 1);
    put_ref(&mut descriptor, 12, 2);
    put_ref(&mut descriptor, 16, 2);
    descriptor[18] = 5;
    descriptor[19] = 5;
    descriptor[20..24].copy_from_slice(&2u32.to_be_bytes());
    descriptor[24..28].copy_from_slice(&2u32.to_be_bytes());
    put_ref(&mut descriptor, 36, 30);
    put_ref(&mut descriptor, 38, 31);
    put_ref(&mut descriptor, 40, 32);
    put_ref(&mut descriptor, 42, 33);
    put_ref(&mut descriptor, 44, 125);
    put_ref(&mut descriptor, 46, 61);
    stream.extend(descriptor);

    let mut data = record(125, 97 + 12 * 8);
    put_ref(&mut data, 2, 61);
    data[90] = b'+';
    data[91..95].copy_from_slice(&12u32.to_be_bytes());
    for (index, value) in [
        0.0, 0.0, 0.0, 0.0, 0.03, 0.0, 0.015, 0.0, 0.0, 0.015, 0.03, 0.0,
    ]
    .into_iter()
    .enumerate()
    {
        put_f64(&mut data, 97 + index * 8, value);
    }
    stream.extend(data);
    stream
}

fn deltas_bspline_surface_wrapper_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&124u16.to_be_bytes());
    stream.extend_from_slice(&10u16.to_be_bytes());
    stream.extend_from_slice(&914u32.to_be_bytes());
    push_reference_run(&mut stream, 5);
    stream.push(b'+');
    for reference in [60u16, 61] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn bspline_curve_replacement_partition_stream() -> Vec<u8> {
    let mut stream = bspline_partition_stream();
    let mut descriptor = record(136, 27);
    put_ref(&mut descriptor, 2, 70);
    put_ref(&mut descriptor, 4, 1);
    put_ref(&mut descriptor, 8, 2);
    put_ref(&mut descriptor, 10, 3);
    put_ref(&mut descriptor, 14, 2);
    descriptor[16] = 5;
    put_ref(&mut descriptor, 23, 42);
    put_ref(&mut descriptor, 25, 43);
    stream.extend(descriptor);

    let mut data = record(135, 15 + 6 * 8);
    put_ref(&mut data, 2, 71);
    data[9..13].copy_from_slice(&6u32.to_be_bytes());
    for (index, value) in [0.0, 0.0, 0.0, 0.02, 0.01, 0.0].into_iter().enumerate() {
        put_f64(&mut data, 15 + index * 8, value);
    }
    stream.extend(data);
    stream
}

fn deltas_bspline_curve_wrapper_stream() -> Vec<u8> {
    let mut stream = DELTAS_PREAMBLE.to_vec();
    stream.extend_from_slice(&134u16.to_be_bytes());
    stream.extend_from_slice(&50u16.to_be_bytes());
    stream.extend_from_slice(&915u32.to_be_bytes());
    push_reference_run(&mut stream, 5);
    stream.push(b'+');
    for reference in [70u16, 71] {
        stream.extend_from_slice(&reference.to_be_bytes());
        stream.push(1);
    }
    stream
}

fn trimmed_topology_partition_stream() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 12);
    let mut trim = record(133, 85);
    put_ref(&mut trim, 2, 12);
    trim[18] = b'+';
    put_ref(&mut trim, 19, 9);
    put_f64(&mut trim, 69, 0.000_25);
    put_f64(&mut trim, 77, 0.000_75);
    // The closed edge's single vertex sits at the trim range's midpoint on the
    // basis line so both trimmed endpoints fall inside the edge's stored
    // 0.3 mm tolerance; the point record is the topology stream's last
    // 40 bytes, before the trim record is appended.
    let point_vec = stream.len() - 40 + 16;
    put_vec3(&mut stream, point_vec, [0.000_5, 0.0, 0.0]);
    stream.extend(trim);
    stream
}

fn mismatched_trimmed_topology_partition_stream() -> Vec<u8> {
    let mut stream = trimmed_topology_partition_stream();
    let point_vec = stream.len() - 85 - 40 + 16;
    put_vec3(&mut stream, point_vec, [0.000_5, 0.01, 0.0]);
    stream
}

fn partnered_trimmed_topology_partition_stream() -> Vec<u8> {
    let mut stream = trimmed_topology_partition_stream();
    let trim = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("trimmed curve");
    put_f64(&mut stream, trim + 69, 0.000_75);
    put_f64(&mut stream, trim + 77, 0.000_25);
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("first face");
    put_ref(&mut stream, face + 18, 20);
    let fin = stream
        .windows(4)
        .position(|window| window == [0, 17, 0, 7])
        .expect("first fin");
    put_ref(&mut stream, fin + 14, 22);
    let first_point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("first point");
    put_vec3(&mut stream, first_point + 16, [0.000_25, 0.0, 0.0]);

    let mut second_face = record(14, 39);
    put_ref(&mut second_face, 2, 20);
    put_f64(&mut second_face, 10, 0.000_2);
    put_ref(&mut second_face, 18, 1);
    put_ref(&mut second_face, 20, 4);
    put_ref(&mut second_face, 22, 21);
    put_ref(&mut second_face, 24, 3);
    put_ref(&mut second_face, 26, 6);
    second_face[28] = b'+';
    stream.extend(second_face);

    let mut second_loop = record(15, 16);
    put_ref(&mut second_loop, 2, 21);
    put_ref(&mut second_loop, 10, 22);
    put_ref(&mut second_loop, 12, 20);
    put_ref(&mut second_loop, 14, 1);
    stream.extend(second_loop);

    let mut second_fin = record(17, 23);
    put_ref(&mut second_fin, 2, 22);
    put_ref(&mut second_fin, 6, 21);
    put_ref(&mut second_fin, 8, 22);
    put_ref(&mut second_fin, 10, 22);
    put_ref(&mut second_fin, 12, 23);
    put_ref(&mut second_fin, 14, 7);
    put_ref(&mut second_fin, 16, 8);
    put_ref(&mut second_fin, 18, 1);
    second_fin[22] = b'-';
    stream.extend(second_fin);

    let mut second_vertex = record(18, 28);
    put_ref(&mut second_vertex, 2, 23);
    put_ref(&mut second_vertex, 16, 24);
    put_f64(&mut second_vertex, 18, 0.000_1);
    stream.extend(second_vertex);

    let mut second_point = record(29, 40);
    put_ref(&mut second_point, 2, 24);
    put_vec3(&mut second_point, 16, [0.000_75, 0.0, 0.0]);
    stream.extend(second_point);
    stream
}

fn forward_trimmed_curve_chain_stream() -> Vec<u8> {
    let mut stream = trimmed_topology_partition_stream();
    let first = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("first trimmed curve");
    put_ref(&mut stream, first + 19, 20);

    let mut second = record(133, 85);
    put_ref(&mut second, 2, 20);
    second[18] = b'+';
    put_ref(&mut second, 19, 9);
    put_f64(&mut second, 69, 0.000_25);
    put_f64(&mut second, 77, 0.000_75);
    stream.extend(second);
    stream
}

fn topology_with_extended_edge_curve_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    stream[edge + 24..edge + 26].copy_from_slice(&(-9i16).to_be_bytes());
    stream.splice(edge + 26..edge + 26, [0, 0]);
    stream
}

fn topology_with_extended_face_attribute_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let face = stream
        .windows(4)
        .position(|window| window == [0, 14, 0, 4])
        .expect("face record");
    stream.splice(face + 8..face + 10, [0xff, 0xff, 0x00, 0x00]);
    stream
}

fn topology_with_extended_edge_attribute_reference() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    stream.splice(edge + 8..edge + 10, [0xff, 0xff, 0x00, 0x00]);
    stream
}

fn topology_with_extended_internal_topology_references() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(13, 3, 8), (15, 5, 8), (17, 7, 4), (18, 10, 8), (29, 11, 8)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        stream.splice(
            record + offset..record + offset + 2,
            [0xff, 0xff, 0x00, 0x00],
        );
    }
    stream
}

fn topology_with_fully_extended_geometry_headers() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt) in [(50, 6), (30, 9)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("geometry record");
        for index in 0..5 {
            let at = record + 8 + index * 4;
            stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
        }
    }
    stream
}

fn topology_with_escaped_geometry_envelopes() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for marker in [[0, 50, 0, 6], [0, 30, 0, 9]] {
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("geometry record");
        stream.insert(record + 2, 0xff);
    }
    stream
}

fn offset_surface_with_fully_extended_common_header() -> Vec<u8> {
    let mut stream = offset_surface_topology_partition_stream();
    let record = stream
        .windows(4)
        .position(|window| window == [0, 60, 0, 12])
        .expect("offset record");
    for index in 0..5 {
        let at = record + 8 + index * 4;
        stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
    }
    stream
}

fn fully_extend_common_header(stream: &mut Vec<u8>, marker: [u8; 4]) {
    let record = stream
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("compact geometry record");
    for index in 0..5 {
        let at = record + 8 + index * 4;
        stream.splice(at..at + 2, [0xff, 0xff, 0x00, 0x00]);
    }
}

fn zlib_compress(raw: &[u8]) -> Vec<u8> {
    // Level 1 emits the `78 01` zlib header NX/Parasolid streams use.
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(1));
    e.write_all(raw).unwrap();
    e.finish().unwrap()
}

fn zlib_compress_at_level(raw: &[u8], level: u32) -> Vec<u8> {
    let mut e = ZlibEncoder::new(Vec::new(), Compression::new(level));
    e.write_all(raw).unwrap();
    e.finish().unwrap()
}

/// Assemble a synthetic single-part `.prt`: the SPLMSSTR header, a HEADER
/// directory with one `/Root/UG_PART/UG_PART` file entry, and a zlib-compressed
/// Parasolid partition stream.
fn single_part_prt() -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(0x06); // version tag
    f.extend_from_slice(&[0x11, 0x22, 0x33]); // u24 file tag
    f.extend_from_slice(&[0, 0, 0, 0]); // +0x0c constant
    f.push(0x00); // +0x10 constant
    f.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // +0x11 footer offset (0 → no footer)
    f.extend_from_slice(&[0, 0]); // pad to 0x19
    assert_eq!(f.len(), 0x19);

    // HEADER directory: one file entry naming the canonical part stream.
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/UG_PART/UG_PART";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    // 16-byte payload: file_offset then size (both u64 LE) — point at the zlib blob.
    let blob = zlib_compress(&partition_stream());
    // The blob will be appended after the directory; compute its offset now.
    let dir_end = f.len() + 16; // after this entry's payload
    let blob_off = dir_end as u64;
    f.extend_from_slice(&blob_off.to_le_bytes());
    f.extend_from_slice(&(blob.len() as u64).to_le_bytes());
    f.extend_from_slice(&blob);
    f
}

fn prt_with_named_payloads(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(0x06);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let mut spans = Vec::new();
    for (name, _) in entries {
        file.extend_from_slice(&(name.len() as u32).to_le_bytes());
        file.extend_from_slice(name.as_bytes());
        spans.push(file.len());
        file.extend_from_slice(&[0; 16]);
    }
    for ((_, payload), span) in entries.iter().zip(spans) {
        let offset = file.len();
        file.extend_from_slice(payload);
        file[span..span + 8].copy_from_slice(&(offset as u64).to_le_bytes());
        file[span + 8..span + 16].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    }
    file
}

fn prt_with_arrangements() -> Vec<u8> {
    prt_with_arrangement_attribute(Some("Model"))
}

fn prt_with_arrangement_attribute(active_name: Option<&str>) -> Vec<u8> {
    let mut arrangements = br#"<Arrangements><Arrangement Default="YES" Name="Model"/><Arrangement Default="NO" Name="Exploded"/></Arrangements>"#.to_vec();
    arrangements.push(0);
    let mut attributes = match active_name {
        Some(active_name) => format!(
            r#"<UgAttributes version="4"><Attribute owner="part" pdmBased="false" utf8title="NX_Arrangement" utf8value="{active_name}" version="3" type="StringAttributeType"/></UgAttributes>"#,
        )
        .into_bytes(),
        None => br#"<UgAttributes version="4"></UgAttributes>"#.to_vec(),
    };
    attributes.push(0);
    prt_with_named_payloads(&[
        (
            "/Root/UG_PART/UG_PART",
            zlib_compress(&topology_partition_stream()),
        ),
        ("/Root/part/arrangements", arrangements),
        ("/Root/part/attrs", attributes),
    ])
}

fn topology_part_prt() -> Vec<u8> {
    prt_with_partition(&topology_partition_stream())
}

fn topology_with_missing_tolerances() -> Vec<u8> {
    let mut stream = topology_partition_stream();
    for (tag, xmt, offset) in [(14, 4, 10), (16, 8, 10), (18, 10, 18)] {
        let marker = [0, tag, 0, xmt];
        let record = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("topology record");
        put_f64(&mut stream, record + offset, -31_415_800_000_000.0);
    }
    stream
}

fn prt_with_partition(stream: &[u8]) -> Vec<u8> {
    let mut f = single_part_prt();
    let compressed = zlib_compress(stream);
    let entry = container::scan_bytes(f.clone()).unwrap().entries.remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, f.len());
    f.truncate(offset as usize);
    f.extend_from_slice(&compressed);
    let size_at = offset as usize - 8;
    f[size_at..size_at + 8].copy_from_slice(&(compressed.len() as u64).to_le_bytes());
    f
}

fn prt_with_streams(streams: &[&[u8]]) -> Vec<u8> {
    let mut file = single_part_prt();
    let entry = container::scan_bytes(file.clone())
        .unwrap()
        .entries
        .remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, file.len());
    file.truncate(offset as usize);
    let payload = streams
        .iter()
        .flat_map(|stream| zlib_compress(stream))
        .collect::<Vec<_>>();
    file.extend_from_slice(&payload);
    let size_at = offset as usize - 8;
    file[size_at..size_at + 8].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    file
}

fn prt_with_indexed_om_section() -> Vec<u8> {
    let mut file = single_part_prt();
    let entry = container::scan_bytes(file.clone())
        .unwrap()
        .entries
        .remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, file.len());
    file.truncate(offset as usize);
    let mut payload = indexed_om_section();
    payload.extend(zlib_compress(&partition_stream()));
    file.extend_from_slice(&payload);
    let size_at = offset as usize - 8;
    file[size_at..size_at + 8].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    file
}

fn prt_with_size_framed_om_section() -> Vec<u8> {
    let mut file = single_part_prt();
    let entry = container::scan_bytes(file.clone())
        .unwrap()
        .entries
        .remove(0);
    let (offset, size) = entry.file_span.unwrap();
    assert_eq!(offset as usize + size as usize, file.len());
    file.truncate(offset as usize);
    let mut payload = size_framed_om_section();
    payload.extend(zlib_compress(&partition_stream()));
    file.extend_from_slice(&payload);
    let size_at = offset as usize - 8;
    file[size_at..size_at + 8].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    file
}

fn large_xmt_headers(stream: &[u8]) -> Vec<u8> {
    let marker = b"SCH_TEST_1_9999\x00";
    let start = stream
        .windows(marker.len())
        .position(|window| window == marker)
        .unwrap()
        + marker.len();
    let lengths = [24, 24, 39, 16, 23, 32, 91, 67, 28, 16, 40];
    let mut out = stream[..start].to_vec();
    let mut pos = start;
    for len in lengths {
        let record = &stream[pos..pos + len];
        let xmt = u16::from_be_bytes([record[2], record[3]]);
        out.extend_from_slice(&record[..2]);
        out.extend_from_slice(&(-(i16::try_from(xmt).unwrap())).to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&record[4..]);
        pos += len;
    }
    out
}

/// A synthetic assembly `.prt`: SPLMSSTR header, an `ExternalReferences` file
/// entry, and no embedded Parasolid stream.
fn assembly_prt() -> Vec<u8> {
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(0x06);
    f.extend_from_slice(&[0, 0, 0]);
    f.extend_from_slice(&[0, 0, 0, 0]);
    f.push(0x00);
    f.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/UG_PART/ExternalReferences";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    f.extend_from_slice(&[0u8; 16]); // opaque directory payload
    f
}

fn assembly_with_external_paths() -> Vec<u8> {
    let payload = b"EXTREFSTREAM\x01\x02\x00\x00\x00\x09\x00child.prt\x0c\x00nested/b.prt";
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(0x06);
    f.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/UG_PART/ExternalReferences";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    let offset = f.len() + 16;
    f.extend_from_slice(&(offset as u64).to_le_bytes());
    f.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    f.extend_from_slice(payload);
    f
}

fn rmfastload_prt() -> Vec<u8> {
    let mut payload = b"UGS::Solid::Topol".to_vec();
    payload.extend_from_slice(&50u32.to_le_bytes());
    for id in 1..=50u32 {
        payload.extend_from_slice(&id.to_le_bytes());
    }
    let mut f = Vec::new();
    f.extend_from_slice(MAGIC);
    f.push(6);
    f.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    f.extend_from_slice(b"HEADER");
    let name = b"/Root/FastLoad/RMFastLoad";
    f.extend_from_slice(&(name.len() as u32).to_le_bytes());
    f.extend_from_slice(name);
    let offset = f.len() + 16;
    f.extend_from_slice(&(offset as u64).to_le_bytes());
    f.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    f.extend(payload);
    f
}

fn many_face_partition_stream(node_id_start: u32) -> Vec<u8> {
    let mut stream = Vec::new();
    stream.extend_from_slice(
        b"PS\x00\x00XX: TRANSMIT FILE (partition) created by modeller\x00SCH_TEST_1_9999\x00",
    );
    let mut body = record(12, 24);
    put_ref(&mut body, 2, 2);
    body[4..8].copy_from_slice(&(node_id_start + 100).to_be_bytes());
    stream.extend(body);
    let mut shell = record(13, 24);
    put_ref(&mut shell, 2, 3);
    shell[4..8].copy_from_slice(&(node_id_start + 101).to_be_bytes());
    put_ref(&mut shell, 8, 1);
    put_ref(&mut shell, 10, 2);
    put_ref(&mut shell, 12, 1);
    put_ref(&mut shell, 14, 300);
    put_ref(&mut shell, 16, 1);
    put_ref(&mut shell, 18, 1);
    put_ref(&mut shell, 20, 4);
    put_ref(&mut shell, 22, 1);
    stream.extend(shell);
    let mut region = record(19, 16);
    put_ref(&mut region, 2, 4);
    stream.extend(region);
    for index in 0..50u16 {
        let mut face = record(14, 39);
        put_ref(&mut face, 2, 300 + index);
        face[4..8].copy_from_slice(&(node_id_start + u32::from(index)).to_be_bytes());
        put_f64(&mut face, 10, 0.000_1);
        put_ref(&mut face, 18, if index == 49 { 1 } else { 301 + index });
        put_ref(&mut face, 20, if index == 0 { 1 } else { 299 + index });
        put_ref(&mut face, 22, 1);
        put_ref(&mut face, 24, 3);
        put_ref(&mut face, 26, 500 + index);
        face[28] = b'+';
        stream.extend(face);
    }
    for index in 0..50u16 {
        let mut plane = record(50, 91);
        put_ref(&mut plane, 2, 500 + index);
        plane[18] = b'+';
        put_vec3(&mut plane, 19, [f64::from(index) * 0.001, 0.0, 0.0]);
        put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
        put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
        stream.extend(plane);
    }
    stream
}

fn prt_with_two_bodies_and_rmfastload() -> Vec<u8> {
    let mut part_payload = zlib_compress(&many_face_partition_stream(1_000));
    part_payload.extend(zlib_compress(&many_face_partition_stream(2_000)));
    let mut rm_payload = b"UGS::Solid::Topol".to_vec();
    rm_payload.extend_from_slice(&50u32.to_le_bytes());
    for id in 1_000..1_050u32 {
        rm_payload.extend_from_slice(&id.to_le_bytes());
    }

    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(6);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let part_name = b"/Root/UG_PART/UG_PART";
    file.extend_from_slice(&(part_name.len() as u32).to_le_bytes());
    file.extend_from_slice(part_name);
    let part_span = file.len();
    file.extend_from_slice(&[0; 16]);
    let rm_name = b"/Root/FastLoad/RMFastLoad";
    file.extend_from_slice(&(rm_name.len() as u32).to_le_bytes());
    file.extend_from_slice(rm_name);
    let rm_span = file.len();
    file.extend_from_slice(&[0; 16]);
    let part_offset = file.len();
    file.extend_from_slice(&part_payload);
    let rm_offset = file.len();
    file.extend_from_slice(&rm_payload);
    file[part_span..part_span + 8].copy_from_slice(&(part_offset as u64).to_le_bytes());
    file[part_span + 8..part_span + 16].copy_from_slice(&(part_payload.len() as u64).to_le_bytes());
    file[rm_span..rm_span + 8].copy_from_slice(&(rm_offset as u64).to_le_bytes());
    file[rm_span + 8..rm_span + 16].copy_from_slice(&(rm_payload.len() as u64).to_le_bytes());
    file
}

fn prt_with_two_active_bodies_and_rmfastload() -> Vec<u8> {
    let mut file = prt_with_two_bodies_and_rmfastload();
    let marker = b"UGS::Solid::Topol";
    let count_at = file
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("RMFastLoad payload")
        + marker.len();
    let ids_at = count_at + 4;
    let tail = file[ids_at + 50 * 4..].to_vec();
    file[count_at..count_at + 4].copy_from_slice(&100u32.to_le_bytes());
    file.truncate(ids_at + 50 * 4);
    for id in 2_000..2_050u32 {
        file.extend_from_slice(&id.to_le_bytes());
    }
    file.extend_from_slice(&tail);
    let directory_size_at = file
        .windows(b"/Root/FastLoad/RMFastLoad".len())
        .position(|window| window == b"/Root/FastLoad/RMFastLoad")
        .expect("RMFastLoad directory")
        + b"/Root/FastLoad/RMFastLoad".len()
        + 8;
    file[directory_size_at..directory_size_at + 8]
        .copy_from_slice(&((marker.len() + 4 + 100 * 4) as u64).to_le_bytes());
    file
}

fn prt_with_missing_active_body_record() -> Vec<u8> {
    let mut active_stream = many_face_partition_stream(1_000);
    let body = active_stream
        .windows(4)
        .position(|window| window == [0, 12, 0, 2])
        .expect("body record");
    active_stream[body..body + 24].fill(0xff);
    let mut part_payload = zlib_compress(&active_stream);
    part_payload.extend(zlib_compress(&many_face_partition_stream(2_000)));
    let mut rm_payload = b"UGS::Solid::Topol".to_vec();
    rm_payload.extend_from_slice(&50u32.to_le_bytes());
    for id in 1_000..1_050u32 {
        rm_payload.extend_from_slice(&id.to_le_bytes());
    }

    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(6);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let part_name = b"/Root/UG_PART/UG_PART";
    file.extend_from_slice(&(part_name.len() as u32).to_le_bytes());
    file.extend_from_slice(part_name);
    let part_span = file.len();
    file.extend_from_slice(&[0; 16]);
    let rm_name = b"/Root/FastLoad/RMFastLoad";
    file.extend_from_slice(&(rm_name.len() as u32).to_le_bytes());
    file.extend_from_slice(rm_name);
    let rm_span = file.len();
    file.extend_from_slice(&[0; 16]);
    let part_offset = file.len();
    file.extend_from_slice(&part_payload);
    let rm_offset = file.len();
    file.extend_from_slice(&rm_payload);
    file[part_span..part_span + 8].copy_from_slice(&(part_offset as u64).to_le_bytes());
    file[part_span + 8..part_span + 16].copy_from_slice(&(part_payload.len() as u64).to_le_bytes());
    file[rm_span..rm_span + 8].copy_from_slice(&(rm_offset as u64).to_le_bytes());
    file[rm_span + 8..rm_span + 16].copy_from_slice(&(rm_payload.len() as u64).to_le_bytes());
    file
}

fn prt_with_weak_rmfastload_overlap() -> Vec<u8> {
    let mut file = prt_with_two_bodies_and_rmfastload();
    let marker = b"UGS::Solid::Topol";
    let payload = file
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("RMFastLoad payload")
        + marker.len()
        + 4;
    for index in 0..50u32 {
        let id = if index < 5 {
            1_000 + index
        } else {
            10_000 + index
        };
        let at = payload + index as usize * 4;
        file[at..at + 4].copy_from_slice(&id.to_le_bytes());
    }
    file
}

#[test]
fn detect_high_on_magic() {
    assert_eq!(NxCodec.detect(MAGIC), Confidence::High);
    assert_eq!(NxCodec.detect(&single_part_prt()), Confidence::High);
    assert_eq!(NxCodec.detect(b"PK\x03\x04 not nx"), Confidence::No);
    // A Creo/Granite .prt shares the extension but not the magic.
    assert_eq!(NxCodec.detect(b"\xe0\x02\xff\xfeGRANITE"), Confidence::No);
}

#[test]
fn container_parses_header_and_directory() {
    let c = container::scan_bytes(single_part_prt()).unwrap();
    assert_eq!(c.version, 0x06);
    assert_eq!(c.file_tag, 0x33_22_11);
    assert!(c
        .entries
        .iter()
        .any(|e| e.name == "/Root/UG_PART/UG_PART" && e.file_span.is_some()));
}

#[test]
fn inspect_reports_bounded_nx_object_model_entities() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let summary = NxCodec.inspect(&mut cur).unwrap();
    assert!(summary.notes.iter().any(|note| {
        note == "NX object model: 1 indexed section(s), 2 bounded entity record(s)"
    }));
}

#[test]
fn decode_retains_typed_nx_numeric_expression() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let expressions = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::Expression>("expressions")
        .unwrap();
    assert_eq!(result.ir.native.namespace("nx").unwrap().version, 155);
    assert_eq!(expressions.len(), 1);
    assert_eq!(expressions[0].object_id, Some(0x102));
    assert_eq!(expressions[0].parameter_index, Some(8));
    assert_eq!(
        expressions[0].qualifier.as_deref(),
        Some("CircularPattern_pattern_Circular_Dir_offset_angle")
    );
    assert_eq!(
        expressions[0].name,
        "p8_CircularPattern_pattern_Circular_Dir_offset_angle"
    );
    assert_eq!(expressions[0].unit, crate::native::ExpressionUnit::Degree);
    assert_eq!(expressions[0].expression, "120");
    assert_eq!(expressions[0].value, Some(120.0));
    assert_eq!(expressions[0].source_entry, "/Root/UG_PART/UG_PART");
    assert!(expressions[0]
        .source_table
        .starts_with("nx:om-entry-0:expression-table#"));
    let declarations = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::ExpressionDeclaration>("expression_declarations")
        .unwrap();
    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].object_id, 0x102);
    assert_eq!(declarations[0].parameter_index, 8);
    assert_eq!(declarations[0].literal.as_deref(), Some("120"));
    assert_eq!(
        expressions[0].declaration.as_deref(),
        Some(declarations[0].id.as_str())
    );
    let parameter = result
        .ir
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == expressions[0].name)
        .unwrap();
    assert_eq!(
        parameter.properties.get("declaration"),
        Some(&declarations[0].id)
    );
    assert_eq!(
        parameter.properties.get("declaration_object_id"),
        Some(&"258".to_string())
    );
    let om_records = result
        .source_fidelity
        .retained_records
        .iter()
        .filter(|record| record.id.starts_with("nx:om-section-"))
        .collect::<Vec<_>>();
    assert_eq!(om_records.len(), 2);
    assert!(om_records.iter().all(|record| {
        record.data.as_ref().is_some_and(|data| {
            data.len() as u64 == record.byte_len
                && cadmpeg_ir::hash::sha256_hex(data) == record.sha256
        })
    }));
    let object_records = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::ObjectRecord>("object_records")
        .unwrap();
    assert_eq!(object_records.len(), 2);
    let headers = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::StoreHeader>("store_headers")
        .unwrap();
    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].version, "NX 2027.3102");
    assert_eq!(headers[0].object_id, Some(0x101));
    assert_eq!(object_records[1].object_id, Some(0x102));
    assert_eq!(
        object_records[1].object_id_source_offset,
        object_records[0]
            .object_id_source_offset
            .map(|offset| offset + 4)
    );
    assert_eq!(expressions[0].record.as_ref(), Some(&object_records[1].id));
    assert_eq!(object_records[1].record_ordinal, 1);
    assert_eq!(
        object_records[0].section_offset,
        object_records[1].section_offset
    );
    assert_eq!(object_records[1].byte_len, om_records[1].byte_len);
    assert_eq!(object_records[1].sha256, om_records[1].sha256);
    assert_eq!(
        object_records[1].dependencies,
        vec![object_records[0].id.clone()]
    );
    assert_eq!(
        object_records[0].dependents,
        vec![object_records[1].id.clone()]
    );
    let strings = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::StringValue>("string_values")
        .unwrap();
    assert_eq!(strings.len(), 1);
    assert_eq!(strings[0].record, object_records[1].id);
    assert_eq!(strings[0].object_id, Some(0x102));
    assert_eq!(strings[0].value, "SKETCH_001");
    let references = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::ObjectReference>("object_references")
        .unwrap();
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].record, object_records[1].id);
    assert_eq!(references[0].object_id, Some(0x102));
    assert_eq!(references[0].value, 0x1234_5678);
    assert_eq!(references[0].target_record, None);
    assert_eq!(
        references[1].kind,
        crate::native::ObjectReferenceKind::RecordOrdinal16
    );
    assert_eq!(references[1].value, 0);
    assert_eq!(
        references[1].target_record.as_ref(),
        Some(&object_records[0].id)
    );
    let handles = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::PersistentHandle>("persistent_handles")
        .unwrap();
    assert_eq!(handles.len(), 1);
    assert_eq!(handles[0].value, 0x1234_5678);
    assert_eq!(handles[0].records, vec![object_records[1].id.clone()]);
    assert_eq!(handles[0].occurrence_count, 1);
    assert!(handles[0].external_records.is_empty());
    assert_eq!(result.ir.model.features.len(), 1);
    assert!(matches!(
        result.ir.model.features[0].definition,
        cadmpeg_ir::features::FeatureDefinition::TreeNode {
            role: cadmpeg_ir::features::FeatureTreeNodeRole::Equations,
            ..
        }
    ));
    assert_eq!(result.ir.model.features[0].suppressed, Some(false));
    assert_eq!(result.ir.model.parameters.len(), 1);
    assert_eq!(result.ir.model.parameters[0].expression, "120");
    let parameter = &result.ir.model.parameters[0];
    assert_eq!(parameter.name, expressions[0].name);
    assert!(matches!(
        parameter.value,
        Some(cadmpeg_ir::features::ParameterValue::Angle(
            cadmpeg_ir::features::Angle(value)
        )) if value == 120_f64.to_radians()
    ));
    assert_eq!(parameter.native_ref.as_ref(), Some(&expressions[0].id));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn nx_part_attributes_require_typed_atomic_xml() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<UgAttributes version="4" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Attribute owner="part" pdmBased="false" title="legacy" utf8title="Material"
    value="legacy-value" utf8value="Steel" version="3" xsi:type="StringAttributeType"/>
</UgAttributes>"#;
    let attributes = crate::native::parse_part_attributes(xml, 7, "/Root/part/attrs", 100)
        .expect("typed attributes");
    assert_eq!(attributes.len(), 1);
    assert_eq!(attributes[0].id, "nx:part-attributes-7:attribute#0");
    assert_eq!(attributes[0].title, "Material");
    assert_eq!(attributes[0].value, "Steel");
    assert_eq!(attributes[0].value_type, "StringAttributeType");
    assert!(!attributes[0].pdm_based);
    assert!(attributes[0].source_offset > 100);

    let mut terminated = xml.to_vec();
    terminated.push(0);
    assert_eq!(
        crate::native::parse_part_attributes(&terminated, 7, "/Root/part/attrs", 100)
            .expect("terminated typed attributes"),
        attributes
    );
    terminated.push(0);
    assert!(
        crate::native::parse_part_attributes(&terminated, 7, "/Root/part/attrs", 100).is_none()
    );

    let malformed = xml
        .windows(b"pdmBased=\"false\"".len())
        .position(|window| window == b"pdmBased=\"false\"")
        .map(|at| {
            let mut malformed = xml.to_vec();
            malformed[at + b"pdmBased=\"".len()..at + b"pdmBased=\"false".len()]
                .copy_from_slice(b"maybe");
            malformed
        })
        .unwrap();
    assert!(crate::native::parse_part_attributes(&malformed, 7, "/Root/part/attrs", 100).is_none());
}

#[test]
fn decode_projects_part_attributes_to_document_attributes() {
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<UgAttributes version="4" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Attribute owner="part" pdmBased="false" utf8title="Material"
    utf8value="Steel" version="3" xsi:type="StringAttributeType"/>
</UgAttributes>"#;
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/attrs", xml.to_vec()),
    ]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.attributes.len(), 1);
    let attribute = &result.ir.model.attributes[0];
    assert_eq!(attribute.name, "Material");
    assert_eq!(
        attribute.target,
        cadmpeg_ir::attributes::AttributeTarget::Document
    );
    assert_eq!(
        attribute.values,
        vec![cadmpeg_ir::attributes::AttributeValue::String(
            "Steel".to_string()
        )]
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_retains_length_framed_nx_class_definition() {
    let mut cur = Cursor::new(prt_with_indexed_om_section());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let classes = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::ClassDefinition>("class_definitions")
        .unwrap();
    assert_eq!(classes.len(), 1);
    assert_eq!(classes[0].name, "UGS::EXP_expression");
    assert_eq!(classes[0].ordinal, 0);
    assert_eq!(classes[0].trailing_code, 0x81);
    assert_eq!(classes[0].source_entry, "/Root/UG_PART/UG_PART");
}

#[test]
fn decode_retains_length_framed_nx_field_definitions() {
    let mut cur = Cursor::new(prt_with_size_framed_om_section());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let fields = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::FieldDefinition>("field_definitions")
        .unwrap();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name, "m_target");
    assert_eq!(fields[0].ordinal, 0);
    assert_eq!(fields[0].registry_suffix, [0x01, 0x02]);
    assert_eq!(fields[1].name, "m_tools");
    assert_eq!(fields[1].trailing_code, 0x81);
    assert!(fields[1].registry_suffix.is_empty());
    assert_eq!(fields[1].source_entry, "/Root/UG_PART/UG_PART");
    let classes = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::ClassDefinition>("class_definitions")
        .unwrap();
    assert_eq!(classes[0].layout_prefix, &[0x81, 0x21]);
    assert_eq!(
        classes[0].schema_fingerprint,
        Some([0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef])
    );
    assert_eq!(classes[0].layout_terminal, Some(0x06));
}

#[test]
fn decode_retains_nx_arrangement_configurations() {
    let mut cur = Cursor::new(prt_with_arrangements());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let configurations = result
        .ir
        .native
        .namespace("nx")
        .expect("NX namespace")
        .arena_as::<crate::native::Configuration>("configurations")
        .unwrap();
    assert_eq!(configurations.len(), 2);
    assert_eq!(configurations[0].name, "Model");
    assert!(configurations[0].is_default);
    assert_eq!(configurations[1].name, "Exploded");
    assert!(!configurations[1].is_default);
    assert_eq!(result.ir.model.configurations.len(), 2);
    assert_eq!(result.ir.model.configurations[0].ordinal, 0);
    assert_eq!(result.ir.model.configurations[0].source_index, Some(0));
    assert_eq!(result.ir.model.configurations[0].name, "Model");
    assert!(result.ir.model.configurations[0].active);
    assert_eq!(
        result.ir.model.configurations[0].bodies.resolved(),
        Some(
            result
                .ir
                .model
                .bodies
                .iter()
                .map(|body| body.id.clone())
                .collect::<Vec<_>>()
                .as_slice()
        )
    );
    assert_eq!(result.ir.model.configurations[1].ordinal, 1);
    assert_eq!(result.ir.model.configurations[1].name, "Exploded");
    assert!(!result.ir.model.configurations[1].active);
    assert!(result.ir.model.configurations[1].bodies.is_unresolved());
    let uses = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ConfigurationAttributeUse>("configuration_attribute_uses")
        .unwrap();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].configuration, configurations[0].id);
    assert_eq!(uses[0].name, "Model");
    assert_eq!(
        result.ir.model.configurations[0].properties["active_attribute_use"],
        uses[0].id
    );
    let attributes = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::PartAttribute>("part_attributes")
        .unwrap();
    let mut mismatch = attributes.clone();
    mismatch[0].value = "Other".to_string();
    assert!(crate::native::configuration_attribute_uses(&configurations, &mismatch).is_empty());
    let mut duplicate = attributes.clone();
    duplicate.push(attributes[0].clone());
    assert!(crate::native::configuration_attribute_uses(&configurations, &duplicate).is_empty());
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn nx_neutral_active_configuration_requires_the_exact_attribute_join() {
    for active_name in [None, Some("Other")] {
        let mut cur = Cursor::new(prt_with_arrangement_attribute(active_name));
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
        let native = result
            .ir
            .native
            .namespace("nx")
            .unwrap()
            .arena_as::<crate::native::Configuration>("configurations")
            .unwrap();
        assert!(native[0].is_default);
        assert!(result
            .ir
            .model
            .configurations
            .iter()
            .all(|configuration| !configuration.active && configuration.bodies.is_unresolved()));
    }
}

#[test]
fn decode_exposes_strict_nx_jpeg_preview_metadata() {
    let preview = [
        0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0xb9,
        0x00, 0xf7, 0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xd9,
    ];
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/images/preview", preview.to_vec()),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let attributes = &result.ir.source.unwrap().attributes;
    assert_eq!(attributes["jpeg_preview_count"], "1");
    assert_eq!(attributes["jpeg_preview_0_width"], "247");
    assert_eq!(attributes["jpeg_preview_0_height"], "185");
    assert_eq!(attributes["jpeg_preview_0_precision"], "8");
    assert_eq!(attributes["jpeg_preview_0_components"], "3");
    assert_eq!(
        attributes["jpeg_preview_0_byte_len"],
        preview.len().to_string()
    );

    let mut malformed = preview;
    malformed[10..12].copy_from_slice(&16u16.to_be_bytes());
    assert!(crate::decode::jpeg_dimensions(&malformed).is_none());
}

#[test]
fn decode_retains_strict_tiff_material_texture_assets() {
    let texture = [b'I', b'I', 42, 0, 8, 0, 0, 0, 0, 0];
    let malformed = [b'I', b'I', 42, 0, 40, 0, 0, 0, 0, 0];
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/materialsTif/AISI Steel 4340", texture.to_vec()),
        ("/Root/materialsTif/Truncated", malformed.to_vec()),
    ]);

    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let assets = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::MaterialTextureAsset>("material_texture_assets")
        .unwrap();

    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].name, "AISI Steel 4340");
    assert_eq!(assets[0].byte_order, "little_endian");
    assert_eq!(assets[0].version, 42);
    assert_eq!(assets[0].first_ifd_offset, 8);
    assert_eq!(assets[0].byte_len, texture.len() as u64);
    assert_eq!(assets[0].sha256, cadmpeg_ir::hash::sha256_hex(&texture));
    assert_eq!(assets[0].source_entry, "/Root/materialsTif/AISI Steel 4340");
}

#[test]
fn decode_joins_qaf_material_names_to_texture_assets() {
    let texture = [b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0];
    let qaf = br#"<?xml version="1.0" encoding="UTF-8"?>
<folderContents>
<folderProperties location="images/preview" unmappedLocation="images/preview"><createTime>2026-07-15T08:00:00</createTime><modifyTime>2026-07-15T08:00:01</modifyTime></folderProperties>
<folderProperties location="materialsTif/unmap$1" unmappedLocation="materialsTif/Carbon Fiber Harness Satin Coated"><createTime>2026-07-15T08:01:00</createTime><modifyTime>2026-07-15T08:02:00</modifyTime></folderProperties>
</folderContents>"#;
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/materialsTif/unmap$1", texture.to_vec()),
        ("/Root/qafmetadata", qaf.to_vec()),
    ]);

    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    let namespace = result.ir.native.namespace("nx").unwrap();
    let assets = namespace
        .arena_as::<crate::native::MaterialTextureAsset>("material_texture_assets")
        .unwrap();
    let catalog = namespace
        .arena_as::<crate::native::MaterialTextureCatalogEntry>("material_texture_catalog_entries")
        .unwrap();

    assert_eq!(assets.len(), 1);
    assert_eq!(catalog.len(), 1);
    assert_eq!(catalog[0].texture_asset, assets[0].id);
    assert_eq!(catalog[0].storage_path, "materialsTif/unmap$1");
    assert_eq!(
        catalog[0].material_path,
        "materialsTif/Carbon Fiber Harness Satin Coated"
    );
    assert_eq!(catalog[0].create_time, "2026-07-15T08:01:00");
    assert_eq!(catalog[0].modify_time, "2026-07-15T08:02:00");
    assert_eq!(catalog[0].source_entry, "/Root/qafmetadata");
}

#[test]
fn decode_rejects_ambiguous_nx_arrangement_table_atomically() {
    for arrangements in [
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/><Arrangement Default="YES" Name="Exploded"/></Arrangements>"#.as_slice(),
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/><Arrangement Default="NO" Name="Model"/></Arrangements>"#.as_slice(),
    ] {
        let file = prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/part/arrangements", arrangements.to_vec()),
        ]);
        let mut cur = Cursor::new(file);
        let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
        assert!(result.ir.native.namespace("nx").is_none_or(|namespace| {
            namespace
                .arena_as::<crate::native::Configuration>("configurations")
                .unwrap()
                .is_empty()
        }));
        assert!(result.ir.model.configurations.is_empty());
    }
}

#[test]
fn decode_rejects_repeated_nx_arrangement_terminators_atomically() {
    let mut arrangements =
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/></Arrangements>"#.to_vec();
    arrangements.extend_from_slice(&[0, 0]);
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/arrangements", arrangements),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.configurations.is_empty());
}

#[test]
fn decode_rejects_duplicate_nx_configuration_stream_paths_atomically() {
    let arrangements =
        br#"<Arrangements><Arrangement Default="YES" Name="Model"/></Arrangements>"#.to_vec();
    let attributes = br#"<UgAttributes version="4"><Attribute owner="part" pdmBased="false" utf8title="NX_Arrangement" utf8value="Model" version="3" type="StringAttributeType"/></UgAttributes>"#.to_vec();
    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/arrangements", arrangements.clone()),
        ("/Root/part/arrangements", arrangements.clone()),
        ("/Root/part/attrs", attributes.clone()),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir.model.configurations.is_empty());

    let file = prt_with_named_payloads(&[
        ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
        ("/Root/part/arrangements", arrangements),
        ("/Root/part/attrs", attributes.clone()),
        ("/Root/part/attrs", attributes),
    ]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir.model.configurations.len(), 1);
    assert!(!result.ir.model.configurations[0].active);
    assert!(result.ir.model.configurations[0].bodies.is_unresolved());
    assert!(result.ir.native.namespace("nx").is_none_or(|namespace| {
        namespace
            .arena_as::<crate::native::PartAttribute>("part_attributes")
            .unwrap()
            .is_empty()
    }));
}

#[test]
fn parasolid_extraction_classifies_partition_and_schema() {
    let f = single_part_prt();
    let streams = parasolid::extract_streams(&f);
    let part = streams
        .iter()
        .find(|s| s.kind == StreamKind::Partition)
        .expect("a partition stream");
    assert_eq!(part.schema.as_deref(), Some("SCH_TEST_1_9999"));
    assert!(part.inflated.starts_with(b"PS\x00\x00"));
}

#[test]
fn parasolid_attribute_definition_requires_declared_printable_name_and_field_record() {
    let mut bytes = vec![0xaa, 0x00, 0x4f, 0xff];
    bytes.extend_from_slice(&16u32.to_be_bytes());
    bytes.extend_from_slice(&0x012au16.to_be_bytes());
    bytes.extend_from_slice(b"SDL/TYSA_DENSITY");
    bytes.extend_from_slice(&[0x00, 0x50, 0x00, 0x00, 0x00, 0x01]);
    bytes.extend_from_slice(&0x012bu16.to_be_bytes());
    bytes.extend_from_slice(&0x0030u16.to_be_bytes());
    bytes.extend_from_slice(&0x0031u16.to_be_bytes());
    bytes.extend_from_slice(&[0x00, 0x00, 0x23, 0x28]);
    let descriptor = [
        0x00, 0x00, 0x00, 0x00, 0x03, 0x06, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    ];
    bytes.extend_from_slice(&descriptor);
    bytes.push(1);
    let definitions = crate::parasolid::attribute_definitions(&bytes);
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].offset, 1);
    assert_eq!(definitions[0].xmt, 0x12a);
    assert_eq!(definitions[0].name, "SDL/TYSA_DENSITY");
    assert_eq!(definitions[0].field_count, 1);
    assert_eq!(definitions[0].field_record_xmt, 0x12b);
    assert_eq!(definitions[0].field_record_references, [0x30, 0x31]);
    assert_eq!(definitions[0].field_record_header_words, [0, 0x2328]);
    assert_eq!(definitions[0].field_descriptor_prefix, descriptor);
    assert_eq!(
        crate::native::parasolid_attribute_field_storage(&definitions[0].field_descriptor_prefix),
        Some(crate::native::ParasolidAttributeFieldStorage::Double)
    );
    assert_eq!(definitions[0].field_codes, [1]);

    let truncated = &bytes[..bytes.len() - 1];
    assert!(crate::parasolid::attribute_definitions(truncated).is_empty());

    bytes[20] = 0;
    assert!(crate::parasolid::attribute_definitions(&bytes).is_empty());
}

#[test]
fn decode_transfers_point_plane_cylinder_line() {
    let mut cur = Cursor::new(single_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.report.geometry_transferred);
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    // Point coordinate is scaled metres → millimetres, byte-exact.
    let p = &result.ir.model.points[0].position;
    assert!((p.x - 62.5).abs() < 1e-6 && (p.z - 12.7).abs() < 1e-6);

    // One plane, one cylinder decoded.
    let planes = result
        .ir
        .model
        .surfaces
        .iter()
        .filter(|s| matches!(s.geometry, SurfaceGeometry::Plane { .. }))
        .count();
    let cyls: Vec<_> = result
        .ir
        .model
        .surfaces
        .iter()
        .filter_map(|s| match &s.geometry {
            SurfaceGeometry::Cylinder { radius, .. } => Some(*radius),
            _ => None,
        })
        .collect();
    assert_eq!(planes, 1);
    assert_eq!(cyls.len(), 1);
    assert!((cyls[0] - 4.05).abs() < 1e-6);
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Plane {
            u_axis: axis,
            ..
        } if axis == Vector3::new(1.0, 0.0, 0.0)
    )));
    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder {
            ref_direction: direction,
            ..
        } if direction == Vector3::new(1.0, 0.0, 0.0)
    )));

    // One line decoded, with a unit direction.
    let lines: Vec<_> = result
        .ir
        .model
        .curves
        .iter()
        .filter(|c| matches!(c.geometry, CurveGeometry::Line { .. }))
        .collect();
    assert_eq!(lines.len(), 1);

    // No topology graph is fabricated; the loss is reported as blocking.
    assert!(result.ir.model.faces.is_empty() && result.ir.model.edges.is_empty());
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| l.category == cadmpeg_ir::report::LossCategory::Topology
            && l.severity == cadmpeg_ir::report::Severity::Blocking));

    // The Parasolid stream is preserved verbatim.
    let unknowns = result.ir.native_unknowns("nx").unwrap();
    assert_eq!(unknowns.len(), 1);
    assert_eq!(result.source_fidelity.retained_records[0].sha256.len(), 64);
    assert_eq!(
        unknowns[0].links,
        ["nx:s0:surf#0", "nx:s0:surf#1", "nx:s0:crv#0",]
    );
    assert_eq!(
        result.source_fidelity.annotations.exactness[&unknowns[0].id.to_string()].fields["links"],
        Exactness::Derived
    );

    // The preserved stream owns partial-decode carriers without fabricating topology.
    let report = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn decode_emits_connected_primitive_brep() {
    let mut cur = Cursor::new(topology_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.regions.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(
        result.ir.model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Sheet
    );
    assert_eq!(
        result.ir.model.faces[0].loops,
        vec![result.ir.model.loops[0].id.clone()]
    );
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.1));
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.3));
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.2));
    assert_eq!(
        result.ir.model.coedges[0].radial_next,
        result.ir.model.coedges[0].id
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| loss.category != cadmpeg_ir::report::LossCategory::Topology));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_emits_offset_surface_construction() {
    let stream = offset_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = result
        .ir
        .model
        .procedural_surfaces
        .first()
        .expect("offset surface");
    let ProceduralSurfaceDefinition::Offset {
        support,
        distance,
        u_sense,
        v_sense,
        extension_flags,
    } = &procedural.definition
    else {
        panic!("offset definition");
    };
    assert_eq!(*distance, 2.5);
    assert_eq!(*u_sense, Some(0));
    assert_eq!(*v_sense, Some(0));
    assert!(extension_flags.is_empty());
    assert_ne!(procedural.surface, *support);
    assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidOffsetSurfaceRecord>("parasolid_offset_surface_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].discriminator, 'V');
    assert!(records[0].true_offset);
    assert_eq!(records[0].support_xmt, 6);
    assert_eq!(records[0].distance, 2.5);
    let carrier = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == procedural.surface)
        .expect("offset carrier");
    assert_eq!(
        carrier
            .source_object
            .as_ref()
            .map(|source| &source.object_id),
        Some(&records[0].id)
    );
    assert!(matches!(
        &carrier.geometry,
        SurfaceGeometry::Procedural { construction } if construction == &procedural.id
    ));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn offset_surface_parameter_solver_preserves_support_parameters() {
    let stream = offset_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result.ir.model.procedural_surfaces[0].surface.clone();
    let expected = Point2::new(12.0, 7.0);
    let point = cadmpeg_ir::eval::model_surface_point(&result.ir, &surface, expected.u, expected.v)
        .unwrap();

    let actual =
        crate::decode::offset_surface_parameters(&result.ir, &surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);
}

#[test]
fn offset_surface_parameter_solver_accepts_a_seed_within_fit_tolerance() {
    let stream = offset_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result.ir.model.procedural_surfaces[0].surface.clone();
    let seed = Point2::new(12.0, 7.0);
    let mut point =
        cadmpeg_ir::eval::model_surface_point(&result.ir, &surface, seed.u, seed.v).unwrap();
    point.x += 0.01;

    let actual = crate::decode::offset_surface_parameters_with_tolerance(
        &result.ir,
        &surface,
        point,
        Some(seed),
        Some(0.02),
    )
    .unwrap();

    assert_eq!(actual, seed);
}

#[test]
fn decode_tracks_fully_extended_offset_common_header() {
    let stream = offset_surface_with_fully_extended_common_header();
    assert_eq!(crate::topology::offset_surfaces(&stream).len(), 1);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = result
        .ir
        .model
        .procedural_surfaces
        .first()
        .expect("offset surface");
    let ProceduralSurfaceDefinition::Offset {
        support, distance, ..
    } = &procedural.definition
    else {
        panic!("offset definition");
    };
    assert_eq!(*distance, 2.5);
    assert_ne!(procedural.surface, *support);
    assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
}

#[test]
fn decode_tracks_fully_extended_compact_geometry_headers() {
    let mut blend = blend_surface_topology_partition_stream();
    fully_extend_common_header(&mut blend, [0, 56, 0, 12]);
    assert_eq!(crate::topology::blend_surfaces(&blend).len(), 1);

    let mut intersection = intersection_curve_topology_partition_stream();
    fully_extend_common_header(&mut intersection, [0, 38, 0, 12]);
    assert_eq!(crate::topology::composite_curves(&intersection).len(), 1);

    let mut surface_curve = surface_curve_topology_partition_stream();
    fully_extend_common_header(&mut surface_curve, [0, 137, 0, 12]);
    let surface_curves = crate::topology::surface_curves(&surface_curve);
    assert_eq!(surface_curves.len(), 1);
    assert_eq!(surface_curves[0].xmt, 12);
    assert_eq!(surface_curves[0].pcurve, 9);

    let mut trimmed = trimmed_topology_partition_stream();
    fully_extend_common_header(&mut trimmed, [0, 133, 0, 12]);
    let trims = crate::topology::trimmed_curves(&trimmed);
    assert_eq!(trims.len(), 1);
    assert_eq!(trims[0].parameters, [0.000_25, 0.000_75]);

    let mut bspline = bspline_partition_stream();
    fully_extend_common_header(&mut bspline, [0, 124, 0, 10]);
    fully_extend_common_header(&mut bspline, [0, 134, 0, 50]);
    let mut cur = Cursor::new(prt_with_partition(&bspline));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .any(|surface| matches!(surface.geometry, SurfaceGeometry::Nurbs(_))));
    assert!(result
        .ir
        .model
        .curves
        .iter()
        .any(|curve| matches!(curve.geometry, CurveGeometry::Nurbs(_))));
}

#[test]
fn intersection_construction_recovers_one_missing_term_from_unique_edge_endpoints() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 25, 1);
    let scan = crate::intersection::scan(&stream);
    assert_eq!(scan.constructions.len(), 1);
    assert_eq!(scan.curves.len(), 1);
    assert_eq!(
        scan.rejected,
        crate::intersection::RejectionCounts::default()
    );
}

#[test]
fn intersection_construction_rejects_missing_term_without_topology_endpoint_match() {
    let mut stream = charted_intersection_with_edge_endpoint_witnesses_stream();
    let intersection = stream
        .windows(4)
        .position(|window| window == [0, 38, 0, 12])
        .expect("intersection record");
    put_ref(&mut stream, intersection + 25, 1);
    let chart = stream
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("chart record");
    put_f64(&mut stream, chart + 60, 0.005);

    let scan = crate::intersection::scan(&stream);
    assert_eq!(scan.constructions.len(), 1);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_start_term, 1);
}

#[test]
fn intersection_auxiliaries_reject_duplicate_identities() {
    fn append_record(stream: &mut Vec<u8>, marker: &[u8], len: usize) {
        let start = stream
            .windows(marker.len())
            .position(|window| window == marker)
            .expect("auxiliary record");
        let duplicate = stream[start..start + len].to_vec();
        stream.extend(duplicate);
    }

    let mut chart = charted_intersection_curve_topology_partition_stream();
    append_record(&mut chart, &[0, 40, 0, 0, 0, 2, 0, 20], 108);
    let scan = crate::intersection::scan(&chart);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_chart, 1);
    assert_eq!(
        crate::intersection::scan_with_auxiliary_replacements(
            &chart,
            &chart[..chart.len() - 108],
            &[&chart[chart.len() - 108..]],
        )
        .curves
        .len(),
        1
    );

    let base_term = charted_intersection_curve_topology_partition_stream();
    let mut term = base_term.clone();
    append_record(&mut term, &[0, 41, 0, 0, 0, 1, 0, 21], 34);
    assert_eq!(crate::intersection::term_use_records(&term).len(), 1);
    let scan = crate::intersection::scan(&term);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_start_term, 1);
    assert_eq!(
        crate::intersection::scan_with_auxiliary_replacements(
            &term,
            &base_term,
            &[&term[base_term.len()..]],
        )
        .curves
        .len(),
        1
    );

    let mut uv = charted_intersection_curve_topology_partition_stream();
    append_record(&mut uv, &[0, 204, 0, 0, 0, 4, 0, 23], 41);
    assert!(crate::intersection::support_uv_records(&uv).is_empty());
    let [curve] = crate::intersection::scan(&uv).curves.try_into().unwrap();
    assert_eq!(curve.support_uv, [None, None]);

    let mut blend_bound = blend_bound_charted_intersection_curve_stream();
    append_record(&mut blend_bound, &[0, 59, 0, 14], 24);
    assert!(crate::intersection::blend_bounds(&blend_bound).is_empty());
}

#[test]
fn intersection_chart_accepts_one_matching_parameter_complement() {
    let ext11 = ext11_charted_intersection_curve_stream();
    let ext11_start = ext11
        .windows(8)
        .position(|window| window == [0, 40, 0, 0, 0, 2, 0, 20])
        .expect("ext11 chart");
    let complement = ext11[ext11_start..ext11_start + 236].to_vec();

    let mut stream = charted_intersection_curve_topology_partition_stream();
    stream.extend_from_slice(&complement);
    let [curve] = crate::intersection::scan(&stream)
        .curves
        .try_into()
        .expect("complemented curve");
    assert_eq!(curve.parameters, [2.0, 5.0]);

    stream.extend_from_slice(&complement);
    let scan = crate::intersection::scan(&stream);
    assert!(scan.curves.is_empty());
    assert_eq!(scan.rejected.missing_chart, 1);
}

#[test]
fn decode_resolves_surface_curve_to_its_basis_curve() {
    let stream = surface_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.edges.len(), 1);
    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidSurfaceCurveRecord>("parasolid_surface_curve_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].surface_xmt, 6);
    assert_eq!(records[0].pcurve_xmt, 9);
    assert_eq!(records[0].original_curve_xmt, 9);
    assert_eq!(records[0].tolerance_to_original, 0.000_01);
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_lifts_pcurve_only_fin_carrier_to_its_surface() {
    let mut stream = pcurve_topology_partition_stream();
    let edge = stream
        .windows(4)
        .position(|window| window == [0, 16, 0, 8])
        .expect("edge record");
    put_ref(&mut stream, edge + 24, 1);
    let surface_curve = stream
        .windows(4)
        .position(|window| window == [0, 137, 0, 25])
        .expect("surface curve");
    put_ref(&mut stream, surface_curve + 23, 1);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result.ir.model.edges[0]
        .curve
        .as_ref()
        .and_then(|id| result.ir.model.curves.iter().find(|curve| &curve.id == id))
        .expect("lifted carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Procedural { .. }));
    let ProceduralCurveDefinition::SurfaceCurve {
        family: cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric,
        context,
    } = &result.ir.model.procedural_curves[0].definition
    else {
        panic!("parametric surface curve");
    };
    assert_eq!(
        context.sides[0].surface,
        Some(result.ir.model.faces[0].surface.clone())
    );
    assert!(context.sides[0].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_rolling_ball_blend_surface() {
    let stream = blend_surface_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let procedural = result
        .ir
        .model
        .procedural_surfaces
        .first()
        .expect("blend surface");
    let ProceduralSurfaceDefinition::Blend {
        supports,
        radius,
        cross_section,
        spine,
        native,
    } = &procedural.definition
    else {
        panic!("blend definition");
    };
    assert_eq!(*cross_section, BlendCrossSection::Circular);
    assert_eq!(
        *radius,
        BlendRadiusLaw::Constant {
            signed_radius: -3.0
        }
    );
    assert_eq!(supports[0].as_ref().map(|side| side.reversed), Some(true));
    assert_eq!(supports[1].as_ref().map(|side| side.reversed), Some(false));
    assert!(spine.is_none());
    assert!(native.is_none());
    assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidBlendSurfaceRecord>("parasolid_blend_surface_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].support_xmts, [6, 6]);
    assert_eq!(records[0].spine_xmt, 1);
    assert_eq!(records[0].offsets, [-3.0, 3.0]);
    assert_eq!(records[0].thumb_weights, [1.0, 1.0]);
    let carrier = result
        .ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == procedural.surface)
        .unwrap();
    assert_eq!(
        carrier
            .source_object
            .as_ref()
            .map(|association| association.object_id.as_str()),
        Some(records[0].id.as_str())
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_blend_with_extended_support_reference() {
    let stream = blend_surface_with_extended_support_reference();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 1);
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.procedural_surfaces[0].surface
    );
}

#[test]
fn decode_binds_blend_ball_centre_spine() {
    let stream = blend_surface_with_intersection_spine();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let ProceduralSurfaceDefinition::Blend { spine, .. } =
        &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("blend definition");
    };
    assert_eq!(
        spine.as_ref(),
        Some(&result.ir.model.procedural_curves[0].curve)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_forward_blend_support_reference() {
    let stream = blend_surface_with_forward_blend_support();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.procedural_surfaces.len(), 2);
    let ProceduralSurfaceDefinition::Blend { supports, .. } =
        &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("blend definition");
    };
    assert_eq!(
        supports[0].as_ref().map(|support| &support.surface),
        Some(&result.ir.model.procedural_surfaces[1].surface)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_intersection_curve_as_connected_carrier() {
    let stream = intersection_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let edge_curve = result.ir.model.edges[0].curve.as_ref().expect("edge curve");
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == edge_curve)
        .expect("intersection carrier");
    assert!(matches!(curve.geometry, CurveGeometry::Unknown { .. }));
    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidIntersectionRecord>("parasolid_intersection_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert!(!records[0].delta_twin);
    assert_eq!(records[0].header_references[0], 1);
    assert_eq!(records[0].construction_references, [6, 6, 1, 1, 1, 1]);
    assert_eq!(
        curve.source_object.as_ref().map(|source| &source.object_id),
        Some(&records[0].id)
    );
    assert_eq!(result.ir.model.procedural_curves.len(), 1);
    assert_eq!(result.ir.model.procedural_curves[0].curve, curve.id);
    assert!(result.report.losses.iter().any(|loss| {
        loss.category == LossCategory::Geometry
            && loss.message.starts_with("1 surface-intersection record(s)")
    }));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_deltas_intersection_data_curve() {
    let stream = deltas_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.procedural_curves.len(), 1);
    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidIntersectionRecord>("parasolid_intersection_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].delta_twin);
    assert_eq!(records[0].header_references[0], 1);
    assert_eq!(records[0].construction_references, [6, 6, 1, 1, 1, 1]);
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.procedural_curves[0].curve)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_reports_status_framed_deltas_records_and_tombstones() {
    let stream = status_framed_deltas_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attributes = &result.ir.source.expect("source metadata").attributes;

    assert_eq!(
        attributes.get("deltas.0.full.FACE").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        attributes
            .get("deltas.0.tombstone.EDGE")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(
        attributes.get("deltas.0.grammar").map(String::as_str),
        Some("status_byte_framed_topology")
    );
}

#[test]
fn decode_accepts_exact_loop_and_rejects_incomplete_fin_deltas() {
    let stream = variable_status_framed_deltas_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attributes = &result.ir.source.expect("source metadata").attributes;

    assert!(!attributes.contains_key("deltas.0.full.FIN"));
    assert_eq!(
        attributes.get("deltas.0.full.LOOP").map(String::as_str),
        Some("1")
    );
}

#[test]
fn deltas_point_normalizes_to_partition_record_framing() {
    let record = crate::deltas::walk(&status_framed_deltas_point_stream())
        .records
        .remove(0);
    let mut expected = crate::tests::record(29, 40);
    put_ref(&mut expected, 2, 50);
    expected[4..8].copy_from_slice(&900u32.to_be_bytes());
    for at in [8, 10, 12, 14] {
        put_ref(&mut expected, at, 1);
    }
    put_vec3(&mut expected, 16, [0.0125, -0.002, 0.004]);
    assert_eq!(record.canonical_bytes, expected);
}

#[test]
fn deltas_intersection_normalizes_before_partition_style_decode() {
    let residual = crate::deltas::procedural_residual(&status_framed_deltas_intersection_stream());
    let intersections = crate::topology::composite_curves(&residual);
    assert_eq!(intersections.len(), 1);
    assert_eq!(intersections[0].xmt, 12);
    assert_eq!(intersections[0].references, [6, 7, 20, 21, 22, 23]);
}

#[test]
fn deltas_offset_surface_normalizes_exact_record_envelope() {
    let stream = deltas_offset_surface_partition_stream();
    let record = crate::deltas::walk(&stream).records.remove(0);
    assert_eq!(record.canonical_bytes.len(), 31);
    assert_eq!(
        crate::topology::offset_surfaces(&record.canonical_bytes)[0].distance,
        4.5
    );

    let mut invalid_status = stream.clone();
    let offset = invalid_status
        .windows(4)
        .position(|window| window == [0, 60, 0, 12])
        .expect("OFFSET_SURF record");
    invalid_status[offset + 28] = 0;
    assert!(!crate::deltas::walk(&invalid_status)
        .records
        .iter()
        .any(|record| record.kind == 60));

    let mut truncated = stream;
    truncated.pop();
    assert!(!crate::deltas::walk(&truncated)
        .records
        .iter()
        .any(|record| record.kind == 60));
}

#[test]
fn deltas_procedural_wrappers_normalize_complete_record_envelopes() {
    for (stream, family, kind, byte_len) in [
        (
            deltas_blend_surface_partition_stream(),
            "BLEND_SURF",
            56,
            66,
        ),
        (
            deltas_trimmed_curve_partition_stream(),
            "TRIMMED_CURVE",
            133,
            85,
        ),
        (deltas_surface_curve_partition_stream(), "SP_CURVE", 137, 33),
    ] {
        let census = crate::deltas::walk(&stream);
        assert_eq!(census.full_counts.get(family), Some(&1));
        let record = census
            .records
            .iter()
            .find(|record| record.kind == kind)
            .expect("procedural wrapper");
        assert_eq!(record.canonical_bytes.len(), byte_len);
        assert!(crate::topology::Graph::parse(&record.canonical_bytes)
            .get(kind as u8, 12)
            .is_some());
    }

    let mut invalid_blend = deltas_blend_surface_partition_stream();
    let blend = invalid_blend
        .windows(4)
        .position(|window| window == [0, 56, 0, 12])
        .expect("BLEND_SURF record");
    invalid_blend[blend + 24] = b'X';
    assert!(!crate::deltas::walk(&invalid_blend)
        .records
        .iter()
        .any(|record| record.kind == 56));
}

#[test]
fn merged_deltas_full_record_replaces_partition_node() {
    let partition = topology_partition_stream();
    let mut deltas = status_framed_deltas_point_stream();
    deltas[2..4].copy_from_slice(&11u16.to_be_bytes());
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    let points = crate::geometry::points(&merged);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].position.x, 12.5);
    assert_eq!(points[0].position.y, -2.0);
    assert_eq!(points[0].position.z, 4.0);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_some());
}

#[test]
fn merged_tombstone_preserves_a_topology_referenced_carrier() {
    let partition = topology_partition_stream();
    let mut tombstone = Vec::new();
    tombstone.extend_from_slice(&29u16.to_be_bytes());
    tombstone.extend_from_slice(&11u16.to_be_bytes());
    tombstone.extend_from_slice(&[0, 1]);
    let census = crate::deltas::walk(&tombstone);
    assert_eq!(census.tombstones.len(), 1);
    assert_eq!(census.tombstones[0].kind, 29);
    assert_eq!(census.tombstones[0].xmt, 11);
    let merged = crate::deltas::merge_full_records(&partition, &tombstone);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_some());
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 10.0);
}

#[test]
fn merged_exact_key_tombstone_removes_unreferenced_partition_node() {
    let mut partition = record(29, 40);
    put_ref(&mut partition, 2, 11);
    put_vec3(&mut partition, 16, [0.01, 0.02, 0.03]);
    let tombstone = [0, 29, 0, 11, 0, 1];
    let merged = crate::deltas::merge_full_records(&partition, &tombstone);
    assert!(crate::topology::Graph::parse(&merged).get(29, 11).is_none());
}

#[test]
fn merged_deltas_uses_last_full_or_tombstone_event() {
    let partition = topology_partition_stream();
    let tombstone = [0, 29, 0, 11, 0, 1];
    let mut full = status_framed_deltas_point_stream();
    full[2..4].copy_from_slice(&11u16.to_be_bytes());

    let mut delete_then_replace = tombstone.to_vec();
    delete_then_replace.extend_from_slice(&full);
    let merged = crate::deltas::merge_full_records(&partition, &delete_then_replace);
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 12.5);

    let mut replace_then_delete = full;
    replace_then_delete.extend_from_slice(&tombstone);
    let merged = crate::deltas::merge_full_records(&partition, &replace_then_delete);
    assert_eq!(crate::geometry::points(&merged)[0].position.x, 10.0);
}

#[test]
fn unmatched_delta_tombstones_follow_exact_last_event_identity() {
    let partition = topology_partition_stream();
    let known = [0, 29, 0, 11, 0, 1];
    let unknown = [0, 29, 0, 99, 0, 1];
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &known),
        0
    );
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &unknown),
        1
    );

    let mut full = status_framed_deltas_point_stream();
    full[2..4].copy_from_slice(&99u16.to_be_bytes());
    let mut add_then_delete = full.clone();
    add_then_delete.extend_from_slice(&unknown);
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &add_then_delete),
        0
    );

    let mut delete_then_add = unknown.to_vec();
    delete_then_add.extend_from_slice(&full);
    assert_eq!(
        crate::deltas::unmatched_terminal_tombstones(&partition, &delete_then_add),
        0
    );
}

#[test]
fn decode_emits_point_added_by_deltas_stream() {
    let mut cur = Cursor::new(prt_with_partition(&deltas_point_partition_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 12.5);
    assert_eq!(result.ir.model.points[0].position.y, -2.0);
    assert_eq!(result.ir.model.points[0].position.z, 4.0);
}

#[test]
fn decode_replaces_partition_point_with_same_xmt_deltas_point() {
    let partition = topology_partition_stream();
    let mut deltas = deltas_point_partition_stream();
    let record = deltas
        .windows(2)
        .rposition(|window| window == 29u16.to_be_bytes())
        .expect("deltas POINT");
    deltas[record + 2..record + 4].copy_from_slice(&11u16.to_be_bytes());
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.points.len(), 1);
    assert_eq!(result.ir.model.points[0].position.x, 12.5);
    assert_eq!(result.ir.model.points[0].position.y, -2.0);
    assert_eq!(result.ir.model.points[0].position.z, 4.0);
}

#[test]
fn decode_preserves_partition_edge_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_edge_partition_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.3));
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_face_and_vertex_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_face_vertex_partition_stream();
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.2));
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.1));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_loop_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_loop_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::Graph::parse(&merged)
            .get(15, 5)
            .and_then(|node| node.u32_at(4)),
        Some(0)
    );
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_shell_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_shell_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::Graph::parse(&merged)
            .get(13, 3)
            .and_then(|node| node.u32_at(4)),
        Some(0)
    );
    let mut cur = Cursor::new(prt_with_streams(&[&partition, &deltas]));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_preserves_partition_fin_topology_over_deltas_history() {
    let partition = topology_partition_stream();
    let deltas = deltas_fin_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(
        result.ir.model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Forward
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_line_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_line_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let CurveGeometry::Line { origin, direction } = result.ir.model.curves[0].geometry else {
        panic!("line");
    };
    assert_eq!(origin, cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0));
    assert_eq!(direction, Vector3::new(0.0, 1.0, 0.0));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_plane_from_status_framed_deltas() {
    let partition = topology_partition_stream();
    let deltas = deltas_plane_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { origin, normal, u_axis }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && normal == Vector3::new(0.0, 1.0, 0.0)
                && u_axis == Vector3::new(1.0, 0.0, 0.0)
    ));
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.surfaces[0].id
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_offset_surface_from_status_framed_deltas() {
    let partition = offset_surface_topology_partition_stream();
    let deltas = deltas_offset_surface_partition_stream();
    let census = crate::deltas::walk(&deltas);
    assert_eq!(census.full_counts.get("OFFSET_SURF"), Some(&1));
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::offset_surfaces(&merged)
            .iter()
            .map(|surface| surface.distance)
            .collect::<Vec<_>>(),
        [4.5]
    );
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let [procedural] = result.ir.model.procedural_surfaces.as_slice() else {
        panic!("one offset surface");
    };
    let ProceduralSurfaceDefinition::Offset { distance, .. } = procedural.definition else {
        panic!("offset surface");
    };
    assert_eq!(distance, 4.5);
    assert_eq!(result.ir.model.faces[0].surface, procedural.surface);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_blend_surface_from_status_framed_deltas() {
    let partition = blend_surface_topology_partition_stream();
    let deltas = deltas_blend_surface_partition_stream();
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    let ProceduralSurfaceDefinition::Blend { radius, .. } =
        &result.ir.model.procedural_surfaces[0].definition
    else {
        panic!("blend surface");
    };
    assert_eq!(
        *radius,
        BlendRadiusLaw::Constant {
            signed_radius: -4.0
        }
    );
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.procedural_surfaces[0].surface
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_trimmed_curve_from_status_framed_deltas() {
    let partition = trimmed_topology_partition_stream();
    let deltas = deltas_trimmed_curve_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::trimmed_curves(&merged)[0].parameters,
        [0.000_3, 0.000_7]
    );
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir.model.edges[0].param_range, Some([0.3, 0.7]));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_surface_curve_from_status_framed_deltas() {
    let partition = surface_curve_topology_partition_stream();
    let deltas = deltas_surface_curve_partition_stream();
    let merged = crate::deltas::merge_full_records(&partition, &deltas);
    assert_eq!(
        crate::topology::surface_curves(&merged)[0].tolerance,
        0.000_02
    );
    let result = NxCodec
        .decode(
            &mut Cursor::new(prt_with_streams(&[&partition, &deltas])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_circle_from_status_framed_deltas() {
    let partition = circle_topology_partition_stream();
    let deltas = deltas_circle_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Circle { center, axis, ref_direction, radius }
            if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_ellipse_from_status_framed_deltas() {
    let partition = ellipse_topology_partition_stream();
    let deltas = deltas_ellipse_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        curve.geometry,
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            && axis == Vector3::new(0.0, 1.0, 0.0)
            && major_direction == Vector3::new(1.0, 0.0, 0.0)
            && major_radius == 30.0
            && minor_radius == 12.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_cylinder_from_status_framed_deltas() {
    let partition = cylinder_topology_partition_stream();
    let deltas = deltas_cylinder_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cylinder { origin, axis, ref_direction, radius }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_cone_from_status_framed_deltas() {
    let partition = cone_topology_partition_stream();
    let deltas = deltas_cone_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Cone { origin, axis, ref_direction, radius, ratio, half_angle }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
                && ratio == 1.0
                && (half_angle - std::f64::consts::FRAC_PI_6).abs() < 1e-12
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_sphere_from_status_framed_deltas() {
    let partition = sphere_topology_partition_stream();
    let deltas = deltas_sphere_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Sphere { center, axis, ref_direction, radius }
            if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && axis == Vector3::new(0.0, 1.0, 0.0)
                && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                && radius == 25.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_torus_from_status_framed_deltas() {
    let partition = torus_topology_partition_stream();
    let deltas = deltas_torus_partition_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        surface.geometry,
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } if center == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
            && axis == Vector3::new(0.0, 1.0, 0.0)
            && ref_direction == Vector3::new(1.0, 0.0, 0.0)
            && major_radius == 40.0
            && minor_radius == 15.0
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_charted_surface_intersection_construction() {
    let stream = charted_intersection_curve_topology_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let terms = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidTermUseRecord>("parasolid_term_use_records")
        .unwrap();
    assert_eq!(terms.len(), 2);
    assert_eq!(terms[0].count, 1);
    assert_eq!(terms[0].form, "L?");
    assert_eq!(terms[0].point, [0.0, 0.0, 0.0]);
    assert_eq!(terms[1].point, [10.0, 0.0, 0.0]);
    assert!(terms
        .iter()
        .all(|term| matches!(term.framing, crate::intersection::TermUseFraming::Direct)));
    let support_uv = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidSupportUvRecord>("parasolid_support_uv_records")
        .unwrap();
    assert_eq!(support_uv.len(), 1);
    assert_eq!(support_uv[0].count, 4);
    assert_eq!(support_uv[0].marker, 2);
    assert_eq!(support_uv[0].values, [0.0, 0.0, 0.01, 0.0]);
    assert!(matches!(
        support_uv[0].framing,
        crate::intersection::SupportUvFraming::Direct
    ));
    let charts = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidChartRecord>("parasolid_chart_records")
        .unwrap();
    assert_eq!(charts.len(), 1);
    assert_eq!(charts[0].count, 2);
    assert_eq!(charts[0].base_parameter, 0.0);
    assert_eq!(charts[0].base_scale, 1.0);
    assert_eq!(charts[0].chart_count, 2);
    assert_eq!(charts[0].chordal_error, 0.000_01);
    assert_eq!(charts[0].angular_error, 0.001);
    assert_eq!(charts[0].points, [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]);
    assert!(matches!(
        charts[0].point_layout,
        crate::intersection::ChartPointLayout::Xyz3
    ));

    let procedural = result
        .ir
        .model
        .procedural_curves
        .first()
        .expect("intersection construction");
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == procedural.curve)
        .expect("solved chart cache");
    let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        panic!("charted NURBS cache");
    };
    assert_eq!(nurbs.degree, 1);
    assert_eq!(nurbs.control_points[0].x, 0.0);
    assert_eq!(nurbs.control_points[1].x, 10.0);
    assert_eq!(procedural.cache_fit_tolerance, Some(0.01));
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &procedural.definition
    else {
        panic!("typed surface intersection");
    };
    assert!(context.sides[0].surface.is_some());
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].surface.is_none());
    assert_eq!(context.parameter_range, [0.0, 0.01]);
    assert!(result.ir.model.coedges[0].pcurves.is_empty());
    assert!(!result.report.losses.iter().any(|loss| {
        loss.category == LossCategory::Geometry
            && loss.message.contains("surface-intersection record(s)")
    }));
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn intersection_pcurve_attachment_requires_face_incidence() {
    let ir = cadmpeg_ir::examples::unit_cube();
    let edge = cadmpeg_ir::ids::EdgeId("synthetic:cube:edge#0".into());
    let surface = ir
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.edge == edge && coedge.id.0.contains("bottom"))
        .and_then(|coedge| {
            let loop_ = ir
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == coedge.owner_loop)?;
            ir.model
                .faces
                .iter()
                .find(|face| face.id == loop_.face)
                .map(|face| face.surface.clone())
        })
        .expect("bottom support surface");
    let pcurve = |end| PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.0, 0.0), end],
        weights: None,
        periodic: false,
    };

    assert!(crate::decode::pcurve_matches_edge(
        &ir,
        &edge,
        &surface,
        &pcurve(Point2::new(10.0, 0.0)),
        None,
    ));
    assert!(!crate::decode::pcurve_matches_edge(
        &ir,
        &edge,
        &surface,
        &pcurve(Point2::new(10.0, 5.0)),
        None,
    ));
}

#[test]
fn decode_derives_analytic_support_uv_without_serialized_values() {
    let stream = charted_intersection_without_uv_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == result.ir.model.procedural_curves[0].curve)
        .expect("intersection carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Nurbs(_)));
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("intersection definition");
    };
    assert!(context.sides[0].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_accepts_intersection_terms_within_chart_tolerance() {
    let stream = charted_intersection_with_approximated_term_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let carrier = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == result.ir.model.procedural_curves[0].curve)
        .expect("intersection carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Nurbs(_)));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_emits_ext11_deltas_intersection_chart() {
    let stream = ext11_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let curve_id = &result.ir.model.procedural_curves[0].curve;
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find(|curve| &curve.id == curve_id)
        .expect("intersection cache");
    let CurveGeometry::Nurbs(nurbs) = &curve.geometry else {
        panic!("NURBS chart cache");
    };
    assert_eq!(nurbs.control_points[1].x, 10.0);
    assert_eq!(nurbs.knots, vec![2.0, 2.0, 5.0, 5.0]);
}

#[test]
fn decode_assigns_ext11_uv_lanes_by_unique_surface_evaluation() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let [Some(PcurveGeometry::Nurbs {
        control_points: first,
        ..
    }), Some(PcurveGeometry::Nurbs {
        control_points: second,
        ..
    })] = context.sides.clone().map(|side| side.pcurve)
    else {
        panic!("two ext11 pcurves");
    };
    assert_eq!(first, [Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)]);
    assert_eq!(second, [Point2::new(0.0, 0.0), Point2::new(0.0, 10.0)]);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn ext11_uv_assignment_eliminates_the_complementary_support_lane() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surfaces = [
        result.ir.model.surfaces[0].id.clone(),
        result.ir.model.surfaces[1].id.clone(),
    ];
    result.ir.model.surfaces[1].geometry = SurfaceGeometry::Unknown { record: None };
    let lanes = [
        Some(vec![[0.0, 0.0], [0.01, 0.0]]),
        Some(vec![[0.0, 0.0], [0.0, 0.01]]),
    ];

    let assigned = crate::decode::assign_ext11_support_uv_to_surfaces(
        &result.ir,
        [&surfaces[0], &surfaces[1]],
        &[
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        0.01,
        &lanes,
    )
    .unwrap();

    assert_eq!(assigned, [lanes[0].clone(), None]);
}

#[test]
fn topology_selects_one_candidate_at_an_ambiguous_record_offset() {
    let mut stream = vec![0; 40];
    stream[..7].copy_from_slice(&[0, 12, 0xff, 0xfe, 0x00, 0x02, 0x01]);
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(graph.of_kind(12).count(), 1);
    assert_eq!(graph.at_pos(0).map(|node| node.xmt), Some(65_536));
}

#[test]
fn trimmed_curves_reject_nonfinite_endpoint_witnesses() {
    let mut stream = trimmed_topology_partition_stream();
    let trim = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("trimmed curve");
    put_f64(&mut stream, trim + 21, f64::NAN);
    assert!(crate::topology::trimmed_curves(&stream).is_empty());

    put_f64(&mut stream, trim + 21, f64::MAX);
    assert!(crate::topology::trimmed_curves(&stream).is_empty());
}

#[test]
fn nurbs_carriers_reject_nonfinite_millimeter_control_points() {
    let mut surface = bspline_partition_stream();
    let payload = surface
        .windows(4)
        .position(|window| window == [0, 125, 0, 21])
        .expect("surface payload");
    put_f64(&mut surface, payload + 97, f64::MAX);
    assert!(crate::nurbs::surfaces(&surface).is_empty());

    let mut curve = bspline_partition_stream();
    let payload = curve
        .windows(4)
        .position(|window| window == [0, 135, 0, 41])
        .expect("curve payload");
    put_f64(&mut curve, payload + 15, f64::MAX);
    assert!(crate::nurbs::curves(&curve).is_empty());

    let descriptor = curve
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    put_ref(&mut curve, descriptor + 10, 2);
    put_f64(&mut curve, payload + 15, f64::MAX);
    put_f64(&mut curve, payload + 31, f64::MIN_POSITIVE);
    assert!(crate::nurbs::pcurves(&curve).is_empty());
}

#[test]
fn nurbs_carriers_reject_invalid_basis_cardinality() {
    let mut surface = bspline_partition_stream();
    let descriptor = surface
        .windows(4)
        .position(|window| window == [0, 126, 0, 20])
        .expect("surface descriptor");
    put_ref(&mut surface, descriptor + 6, 2);
    assert!(crate::nurbs::surfaces(&surface).is_empty());

    let mut curve = bspline_partition_stream();
    let descriptor = curve
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    put_ref(&mut curve, descriptor + 4, 2);
    assert!(crate::nurbs::curves(&curve).is_empty());

    put_ref(&mut curve, descriptor + 10, 2);
    assert!(crate::nurbs::pcurves(&curve).is_empty());

    let mut short_knots = bspline_partition_stream();
    let multiplicities = short_knots
        .windows(12)
        .position(|record| record[..2] == [0, 127] && record[6..8] == 42u16.to_be_bytes())
        .expect("curve multiplicities");
    put_ref(&mut short_knots, multiplicities + 10, 1);
    assert!(crate::nurbs::curves(&short_knots).is_empty());
}

#[test]
fn nurbs_carriers_reject_duplicate_support_identities() {
    fn duplicate_record(stream: &mut Vec<u8>, tag: u8, xmt_offset: usize, xmt: u16, len: usize) {
        let start = stream
            .windows(len)
            .position(|record| {
                record[..2] == [0, tag] && record[xmt_offset..xmt_offset + 2] == xmt.to_be_bytes()
            })
            .expect("support record");
        let duplicate = stream[start..start + len].to_vec();
        stream.extend(duplicate);
    }

    for (tag, xmt_offset, xmt, len) in [
        (126, 2, 20, 48),
        (125, 2, 21, 193),
        (127, 6, 30, 12),
        (128, 6, 32, 24),
    ] {
        let mut stream = bspline_partition_stream();
        duplicate_record(&mut stream, tag, xmt_offset, xmt, len);
        assert!(
            crate::nurbs::surfaces(&stream).is_empty(),
            "duplicate type {tag}"
        );
    }

    for (tag, xmt_offset, xmt, len) in [
        (136, 2, 40, 27),
        (135, 2, 41, 63),
        (127, 6, 42, 12),
        (128, 6, 43, 24),
    ] {
        let mut stream = bspline_partition_stream();
        duplicate_record(&mut stream, tag, xmt_offset, xmt, len);
        assert!(
            crate::nurbs::curves(&stream).is_empty(),
            "duplicate type {tag}"
        );
    }
}

#[test]
fn nurbs_decodes_descriptors_at_the_stream_boundary() {
    fn move_record_to_end(stream: &mut Vec<u8>, tag: u8, xmt: u16, len: usize) {
        let start = stream
            .windows(len)
            .position(|record| record[..2] == [0, tag] && record[2..4] == xmt.to_be_bytes())
            .expect("descriptor record");
        let record = stream.drain(start..start + len).collect::<Vec<_>>();
        stream.extend(record);
    }

    let mut surface = bspline_partition_stream();
    move_record_to_end(&mut surface, 126, 20, 48);
    assert_eq!(crate::nurbs::surfaces(&surface).len(), 1);

    let mut curve = bspline_partition_stream();
    move_record_to_end(&mut curve, 136, 40, 27);
    assert_eq!(crate::nurbs::curves(&curve).len(), 1);
}

#[test]
fn intersection_chart_rejects_nonfinite_millimeter_tolerance() {
    let mut stream = charted_intersection_curve_topology_partition_stream();
    let chart = stream
        .windows(2)
        .position(|window| window == [0, 40])
        .expect("chart record");
    put_f64(&mut stream, chart + 28, f64::MAX);
    assert!(crate::intersection::curves(&stream).is_empty());
}

#[test]
fn data_block_object_frame_ids_include_the_store_qualifier() {
    let first = crate::native::data_block_object_frame_id("nx:om-data-blocks-2:block#17", 0);
    let second = crate::native::data_block_object_frame_id("nx:om-data-blocks-3:block#17", 0);
    assert_eq!(first, "nx:om-data-block-object-frames-2:block-frame#17-0");
    assert_eq!(second, "nx:om-data-block-object-frames-3:block-frame#17-0");
    assert_ne!(first, second);
}

#[test]
fn decode_replaces_ambiguous_ext11_uv_lanes_from_analytic_supports() {
    let stream = two_support_ext11_charted_intersection_curve_stream(true);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_completes_one_non_sentinel_ext11_uv_lane_analytically() {
    let stream = partial_ext11_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn completed_intersection_support_lane_attaches_after_topology_emission() {
    let mut ir = cadmpeg_ir::examples::unit_cube();
    let edge = cadmpeg_ir::ids::EdgeId("synthetic:cube:edge#0".into());
    let target = ir
        .model
        .coedges
        .iter_mut()
        .find(|coedge| coedge.edge == edge && coedge.id.0.contains("bottom"))
        .expect("bottom coedge");
    target.id = cadmpeg_ir::ids::CoedgeId("nx:s0:fin#42".into());
    target.pcurves.clear();
    let owner_loop = target.owner_loop.clone();
    let surface = ir
        .model
        .loops
        .iter()
        .find(|loop_| loop_.id == owner_loop)
        .and_then(|loop_| {
            ir.model
                .faces
                .iter()
                .find(|face| face.id == loop_.face)
                .map(|face| face.surface.clone())
        })
        .expect("bottom support");
    let curve = ir
        .model
        .edges
        .iter()
        .find(|candidate| candidate.id == edge)
        .and_then(|edge| edge.curve.clone())
        .expect("edge curve");
    ir.model
        .procedural_curves
        .push(cadmpeg_ir::geometry::ProceduralCurve {
            id: cadmpeg_ir::ids::ProceduralCurveId("nx:test:intersection#0".into()),
            curve,
            definition: ProceduralCurveDefinition::Intersection {
                context: cadmpeg_ir::geometry::IntcurveSupportContext {
                    sides: [
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: Some(surface),
                            pcurve: Some(PcurveGeometry::Nurbs {
                                degree: 1,
                                knots: vec![0.0, 0.0, 1.0, 1.0],
                                control_points: vec![Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)],
                                weights: None,
                                periodic: false,
                            }),
                        },
                        cadmpeg_ir::geometry::IntcurveSupportSide {
                            surface: None,
                            pcurve: None,
                        },
                    ],
                    parameter_range: [0.0, 1.0],
                    discontinuities: [Vec::new(), Vec::new(), Vec::new()],
                },
                discontinuity_flag: false,
            },
            cache_fit_tolerance: None,
        });
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let source_stream = annotations.stream("nx:test");

    crate::decode::attach_completed_intersection_pcurves(
        &mut ir,
        &crate::topology::Graph::parse(&[]),
        "nx:s0",
        source_stream,
        &mut annotations,
    );

    let completed = ir
        .model
        .pcurves
        .iter()
        .find(|pcurve| pcurve.id.0.contains("intersection-pcurve-completed"))
        .expect("validated completed support lane attaches");
    assert!(ir.model.coedges.iter().any(|coedge| coedge
        .pcurves
        .iter()
        .any(|pcurve| pcurve.pcurve == completed.id)));
}

#[test]
fn ext11_uv_completion_runs_after_support_incidence_resolution() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural_id = result.ir.model.procedural_curves[0].id.clone();
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    for side in &mut context.sides {
        side.pcurve = None;
    }
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        [
            Some(vec![[0.0, 0.0], [0.01, 0.0]]),
            Some(vec![[0.0, 0.0], [0.0, 0.01]]),
        ],
    )];

    crate::decode::complete_ext11_support_uv(&mut result.ir, &pending);

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn analytic_uv_completion_fills_missing_intersection_support_lanes() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural_id = result.ir.model.procedural_curves[0].id.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    for side in &mut context.sides {
        side.pcurve = None;
    }
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        [None, None],
    )];

    crate::decode::complete_support_uv(&mut result.ir, &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides.iter().all(|side| side.pcurve.is_some()));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn support_uv_completion_closes_blend_spine_dependencies_to_a_fixed_point() {
    use cadmpeg_ir::geometry::{BlendSupport, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{ProceduralCurveId, ProceduralSurfaceId, SurfaceId};

    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let spine_id = result.ir.model.procedural_curves[0].id.clone();
    let spine_curve = result.ir.model.procedural_curves[0].curve.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let spine_surfaces = context
        .sides
        .each_ref()
        .map(|side| side.surface.clone().unwrap());
    let radius = 2.0;
    let offset_surfaces = [0usize, 1usize].map(|side| {
        let support = result
            .ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == spine_surfaces[side])
            .unwrap();
        let SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } = support.geometry
        else {
            panic!("plane support");
        };
        let id = SurfaceId(format!("synthetic:offset-support-{side}"));
        result.ir.model.surfaces.push(Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(
                    origin.x + radius * normal.x,
                    origin.y + radius * normal.y,
                    origin.z + radius * normal.z,
                ),
                normal,
                u_axis,
            },
            source_object: None,
        });
        id
    });
    let blend = SurfaceId("synthetic:dependent-blend".into());
    let blend_construction = ProceduralSurfaceId("synthetic:dependent-blend-definition".into());
    result.ir.model.surfaces.push(Surface {
        id: blend.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: blend_construction.clone(),
        },
        source_object: None,
    });
    result.ir.model.procedural_surfaces.push(ProceduralSurface {
        id: blend_construction,
        surface: blend.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: offset_surfaces.map(|surface| {
                Some(BlendSupport {
                    surface,
                    reversed: false,
                })
            }),
            spine: Some(spine_curve),
            radius: BlendRadiusLaw::Constant {
                signed_radius: radius,
            },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
    });
    let parameters = vec![0.0, 0.01];
    let points = parameters
        .iter()
        .map(|parameter| {
            crate::decode::blend_surface_point(&result.ir, &blend, *parameter, 0.5).unwrap()
        })
        .collect::<Vec<_>>();

    let dependent_id = ProceduralCurveId("synthetic:dependent-intersection".into());
    let mut dependent = result.ir.model.procedural_curves[0].clone();
    dependent.id = dependent_id.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } = &mut dependent.definition else {
        unreachable!()
    };
    context.sides[0].surface = Some(blend);
    context.sides[0].pcurve = None;
    context.sides[1].surface = None;
    context.sides[1].pcurve = None;
    result.ir.model.procedural_curves.insert(0, dependent);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[1].definition
    else {
        unreachable!()
    };
    for side in &mut context.sides {
        side.pcurve = None;
    }
    let pending = vec![
        (dependent_id, points, parameters.clone(), 0.01, [None, None]),
        (
            spine_id,
            vec![
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
            ],
            parameters,
            0.01,
            [None, None],
        ),
    ];

    crate::decode::complete_support_uv(&mut result.ir, &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        unreachable!()
    };
    assert!(context.sides[0].pcurve.is_some());
}

#[test]
fn analytic_uv_completion_replaces_a_sentinel_contaminated_support_lane() {
    let stream = two_support_ext11_charted_intersection_curve_stream(false);
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let mut result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let procedural_id = result.ir.model.procedural_curves[0].id.clone();
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &mut result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let Some(PcurveGeometry::Nurbs { control_points, .. }) = context.sides[0].pcurve.as_mut()
    else {
        panic!("NURBS support lane");
    };
    control_points[1] = Point2::new(
        crate::decode::MISSING_TOLERANCE,
        crate::decode::MISSING_TOLERANCE,
    );
    let pending = vec![(
        procedural_id,
        vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
        ],
        vec![0.0, 0.01],
        0.01,
        [None, None],
    )];

    crate::decode::complete_support_uv(&mut result.ir, &pending);

    let ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let Some(PcurveGeometry::Nurbs { control_points, .. }) = context.sides[0].pcurve.as_ref()
    else {
        panic!("NURBS support lane");
    };
    assert!(control_points.iter().all(|point| {
        point.u.to_bits() != crate::decode::MISSING_TOLERANCE.to_bits()
            && point.v.to_bits() != crate::decode::MISSING_TOLERANCE.to_bits()
    }));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn equivalent_offset_supports_share_a_complete_parameter_lane() {
    use cadmpeg_ir::geometry::{ProceduralCurve, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::{Point3, Vector3};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let supports = [SurfaceId("support-a".into()), SurfaceId("support-b".into())];
    for support in &supports {
        ir.model.surfaces.push(Surface {
            id: support.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        });
    }
    let offsets = [SurfaceId("offset-a".into()), SurfaceId("offset-b".into())];
    for (ordinal, (surface, support)) in offsets.iter().zip(&supports).enumerate() {
        let construction = ProceduralSurfaceId(format!("offset-construction-{ordinal}"));
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: construction,
            surface: surface.clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: support.clone(),
                distance: 30.0,
                u_sense: Some(0),
                v_sense: Some(0),
                extension_flags: Vec::new(),
            },
            cache_fit_tolerance: None,
        });
    }
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("intersection".into()),
        curve: CurveId("curve".into()),
        definition: ProceduralCurveDefinition::Intersection {
            context: cadmpeg_ir::geometry::IntcurveSupportContext {
                sides: [
                    cadmpeg_ir::geometry::IntcurveSupportSide {
                        surface: Some(offsets[0].clone()),
                        pcurve: None,
                    },
                    cadmpeg_ir::geometry::IntcurveSupportSide {
                        surface: Some(offsets[1].clone()),
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(1.0, 2.0),
                            direction: Point2::new(3.0, 4.0),
                        }),
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });

    assert!(crate::decode::parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
    crate::decode::complete_parameterization_equivalent_support_uv(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves[0].definition
    else {
        panic!("intersection");
    };
    assert_eq!(context.sides[0].pcurve, context.sides[1].pcurve);

    let ProceduralSurfaceDefinition::Offset { distance, .. } =
        &mut ir.model.procedural_surfaces[1].definition
    else {
        unreachable!()
    };
    *distance = 31.0;
    assert!(!crate::decode::parameterization_equivalent_surfaces(
        &ir,
        &offsets[0],
        &offsets[1]
    ));
}

#[test]
fn nurbs_parameter_solver_inverts_a_rational_surface_point() {
    let surface = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(0.0, 10.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 0.0, 0.0),
            cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
        ],
        weights: Some(vec![1.0, 2.0, 3.0, 4.0]),
        u_periodic: false,
        v_periodic: false,
    };
    let expected = Point2::new(0.37, 0.61);
    let point = cadmpeg_ir::eval::nurbs_surface_point(&surface, expected.u, expected.v).unwrap();

    let actual = crate::decode::nurbs_parameters(&surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-10);
    assert!((actual.v - expected.v).abs() < 1.0e-10);

    let after_invalid_seed =
        crate::decode::nurbs_parameters(&surface, point, Some(Point2::new(f64::NAN, 0.5))).unwrap();
    assert!((after_invalid_seed.u - expected.u).abs() < 1.0e-10);
    assert!((after_invalid_seed.v - expected.v).abs() < 1.0e-10);
}

#[test]
fn surface_intersection_continuation_corrects_a_chart_selected_branch() {
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let first = SurfaceId("synthetic:first-intersection-plane".into());
    let second = SurfaceId("synthetic:second-intersection-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: first.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: second.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    let chart = vec![
        Point3::new(1.0e-4, -2.0e-4, 0.0),
        Point3::new(-1.0e-4, 2.0e-4, 2.0),
        Point3::new(2.0e-4, 1.0e-4, 5.0),
    ];
    let lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &second],
        &chart,
        1.0e-3,
    )
    .unwrap();
    assert_eq!(lanes[0].len(), chart.len());
    for (ordinal, expected_z) in [0.0, 2.0, 5.0].into_iter().enumerate() {
        let first_point = cadmpeg_ir::eval::model_surface_point(
            &ir,
            &first,
            lanes[0][ordinal].u,
            lanes[0][ordinal].v,
        )
        .unwrap();
        let second_point = cadmpeg_ir::eval::model_surface_point(
            &ir,
            &second,
            lanes[1][ordinal].u,
            lanes[1][ordinal].v,
        )
        .unwrap();
        assert!((first_point.x - second_point.x).abs() < 1.0e-10);
        assert!((first_point.y - second_point.y).abs() < 1.0e-10);
        assert!((first_point.z - second_point.z).abs() < 1.0e-10);
        assert!((first_point.z - expected_z).abs() < 1.0e-10);
    }

    let off_branch = [chart[0], Point3::new(1.0, 1.0, 2.0)];
    assert!(crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &second],
        &off_branch,
        1.0e-3,
    )
    .is_none());
    assert!(crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&first, &first],
        &chart,
        1.0e-3,
    )
    .is_none());

    let cylinder = SurfaceId("synthetic:intersection-cylinder".into());
    let section_plane = SurfaceId("synthetic:intersection-section-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: cylinder.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 0.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        },
        Surface {
            id: section_plane.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let circular_chart =
        [0.0_f64, 0.3, 0.8].map(|angle| Point3::new(2.0 * angle.cos(), 2.0 * angle.sin(), 1.0e-5));
    let circular_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&cylinder, &section_plane],
        &circular_chart,
        1.0e-3,
    )
    .unwrap();
    for (cylinder_uv, plane_uv) in circular_lanes[0].iter().zip(&circular_lanes[1]) {
        let cylinder_point =
            cadmpeg_ir::eval::model_surface_point(&ir, &cylinder, cylinder_uv.u, cylinder_uv.v)
                .unwrap();
        let plane_point =
            cadmpeg_ir::eval::model_surface_point(&ir, &section_plane, plane_uv.u, plane_uv.v)
                .unwrap();
        assert!((cylinder_point.x - plane_point.x).abs() < 1.0e-8);
        assert!((cylinder_point.y - plane_point.y).abs() < 1.0e-8);
        assert!((cylinder_point.z - plane_point.z).abs() < 1.0e-8);
    }

    let tangent_cylinder = SurfaceId("synthetic:tangent-cylinder".into());
    let tangent_plane = SurfaceId("synthetic:tangent-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: tangent_cylinder.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(0.0, 0.0, 1.0),
                axis: Vector3::new(0.0, 1.0, 0.0),
                ref_direction: Vector3::new(0.0, 0.0, -1.0),
                radius: 1.0,
            },
            source_object: None,
        },
        Surface {
            id: tangent_plane.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let tangent_chart = [0.0, 1.0, 3.0, 6.0].map(|y| Point3::new(0.0, y, 0.0));
    let tangent_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&tangent_cylinder, &tangent_plane],
        &tangent_chart,
        1.0e-8,
    )
    .unwrap();
    for (ordinal, y) in [0.0, 1.0, 3.0, 6.0].into_iter().enumerate() {
        assert!((tangent_lanes[0][ordinal].v - y).abs() < 1.0e-10);
        assert!((tangent_lanes[1][ordinal].v - y).abs() < 1.0e-10);
    }

    let seam_chart = [3.0_f64, 3.1, 3.2, 3.3]
        .map(|angle| Point3::new(2.0 * angle.cos(), 2.0 * angle.sin(), 1.0e-5));
    let seam_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&cylinder, &section_plane],
        &seam_chart,
        1.0e-3,
    )
    .unwrap();
    assert!(seam_lanes[0].windows(2).all(|pair| pair[0].u < pair[1].u));
    assert!(seam_lanes[0].last().unwrap().u > std::f64::consts::PI);

    let periodic_nurbs = SurfaceId("synthetic:periodic-nurbs-prism".into());
    let nurbs_section = SurfaceId("synthetic:periodic-nurbs-section".into());
    let periodic_geometry = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 5,
        v_count: 2,
        control_points: [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 0.0)]
            .into_iter()
            .flat_map(|(x, y)| [Point3::new(x, y, 0.0), Point3::new(x, y, 1.0)])
            .collect(),
        weights: None,
        u_periodic: true,
        v_periodic: false,
    };
    ir.model.surfaces.extend([
        Surface {
            id: periodic_nurbs.clone(),
            geometry: SurfaceGeometry::Nurbs(periodic_geometry.clone()),
            source_object: None,
        },
        Surface {
            id: nurbs_section.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.5),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ]);
    let nurbs_chart = [3.8, 3.9, 4.1, 4.2]
        .map(|u| cadmpeg_ir::eval::nurbs_surface_point(&periodic_geometry, u, 0.5).unwrap());
    let nurbs_lanes = crate::decode::continue_surface_intersection_parameters(
        &ir,
        [&periodic_nurbs, &nurbs_section],
        &nurbs_chart,
        1.0e-8,
    )
    .unwrap();
    assert!(nurbs_lanes[0].windows(2).all(|pair| pair[0].u < pair[1].u));
    assert!(nurbs_lanes[0].last().unwrap().u > 4.0);
}

#[test]
fn periodic_surface_lookup_rejects_a_cyclic_offset_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{ProceduralSurfaceId, SurfaceId};

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let surfaces = [SurfaceId("cycle-a".into()), SurfaceId("cycle-b".into())];
    let constructions = [
        ProceduralSurfaceId("cycle-construction-a".into()),
        ProceduralSurfaceId("cycle-construction-b".into()),
    ];
    for side in 0..2 {
        ir.model.surfaces.push(Surface {
            id: surfaces[side].clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: constructions[side].clone(),
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: constructions[side].clone(),
            surface: surfaces[side].clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: surfaces[1 - side].clone(),
                distance: 1.0,
                u_sense: Some(0),
                v_sense: Some(0),
                extension_flags: Vec::new(),
            },
            cache_fit_tolerance: None,
        });
    }

    assert_eq!(
        crate::decode::surface_parameter_periods(&ir, &surfaces[0]),
        [None, None]
    );
}

#[test]
fn nurbs_parameter_solver_rejects_a_remote_local_minimum_seed() {
    let mut control_points = Vec::new();
    for (x, z) in [
        (-10.0, 0.0),
        (0.0, 0.0),
        (10.0, 2.0),
        (0.0, 4.0),
        (-10.0, 4.0),
    ] {
        control_points.extend([
            cadmpeg_ir::math::Point3::new(x, 0.0, z),
            cadmpeg_ir::math::Point3::new(x, 10.0, z),
        ]);
    }
    let surface = cadmpeg_ir::geometry::NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 5,
        v_count: 2,
        control_points,
        weights: None,
        u_periodic: false,
        v_periodic: false,
    };
    let expected = Point2::new(0.125, 0.3);
    let point = cadmpeg_ir::eval::nurbs_surface_point(&surface, expected.u, expected.v).unwrap();

    let actual =
        crate::decode::nurbs_parameters(&surface, point, Some(Point2::new(0.875, 0.3))).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-10);
    assert!((actual.v - expected.v).abs() < 1.0e-10);
}

#[test]
fn nurbs_curve_closest_parameter_does_not_trust_a_remote_seed() {
    use cadmpeg_ir::geometry::{Curve, NurbsCurve};
    use cadmpeg_ir::ids::CurveId;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let curve = CurveId("synthetic:piecewise-spine".into());
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Nurbs(NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
            control_points: vec![
                cadmpeg_ir::math::Point3::new(-10.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Point3::new(10.0, 10.0, 0.0),
            ],
            weights: None,
            periodic: false,
        }),
        source_object: None,
    });

    let actual = crate::decode::closest_spine_parameter(
        &ir,
        &curve,
        cadmpeg_ir::math::Point3::new(-5.0, 2.0, 0.0),
        Some(0.9),
    )
    .unwrap();

    assert!((actual - 0.25).abs() < 1.0e-10);
}

#[test]
fn spine_contact_pcurve_inverts_linear_and_rational_support_parameters() {
    let pcurve = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![2.0, 2.0, 5.0, 9.0, 9.0],
        control_points: vec![
            Point2::new(-1.0, 3.0),
            Point2::new(2.0, 6.0),
            Point2::new(6.0, 4.0),
        ],
        weights: None,
        periodic: false,
    };

    let first = crate::decode::closest_pcurve_parameter(&pcurve, Point2::new(0.5, 4.5)).unwrap();
    let second = crate::decode::closest_pcurve_parameter(&pcurve, Point2::new(5.0, 4.5)).unwrap();

    assert!((first - 3.5).abs() < 1.0e-12);
    assert!((second - 8.0).abs() < 1.0e-12);

    let rational = PcurveGeometry::Nurbs {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 1.0],
        control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        weights: Some(vec![1.0, 2.0]),
        periodic: false,
    };
    let rational_parameter =
        crate::decode::closest_pcurve_parameter(&rational, Point2::new(0.5, 0.0)).unwrap();
    assert!((rational_parameter - 1.0 / 3.0).abs() < 1.0e-10);

    let quadratic = PcurveGeometry::Nurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 1.0),
            Point2::new(2.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    let quadratic_parameter =
        crate::decode::closest_pcurve_parameter(&quadratic, Point2::new(1.0, 0.5)).unwrap();
    assert!((quadratic_parameter - 0.5).abs() < 1.0e-10);
}

#[test]
fn blend_contact_offset_requires_the_radius_magnitude() {
    assert!(crate::decode::blend_contact_offset_matches(2.0, 5.0, 3.0));
    assert!(crate::decode::blend_contact_offset_matches(2.0, -1.0, 3.0));
    assert!(crate::decode::blend_contact_offset_matches(
        2.0,
        f64::from_bits(5.0f64.to_bits() + 1),
        3.0,
    ));
    assert!(!crate::decode::blend_contact_offset_matches(
        2.0, 5.001, 3.0
    ));
}

#[test]
fn blend_contact_matches_separate_analytic_offset_carriers() {
    use cadmpeg_ir::geometry::Surface;
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let support = SurfaceId("synthetic:support-cylinder".into());
    let offset = SurfaceId("synthetic:offset-cylinder".into());
    let cylinder = |id, radius| Surface {
        id,
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(-46.75, 0.0, -112.06),
            axis: Vector3::new(1.0, 0.0, 0.0),
            ref_direction: Vector3::new(0.0, 0.0, -1.0),
            radius,
        },
        source_object: None,
    };
    ir.model.surfaces.extend([
        cylinder(support.clone(), 294.0),
        cylinder(offset.clone(), 299.0),
    ]);

    assert_eq!(
        crate::decode::constant_surface_offset_between(&ir, &support, &offset, 0),
        Some(5.0)
    );
    let SurfaceGeometry::Cylinder { origin, .. } = &mut ir.model.surfaces[1].geometry else {
        unreachable!()
    };
    origin.y = 1.0;
    assert!(crate::decode::constant_surface_offset_between(&ir, &support, &offset, 0).is_none());

    let support_plane = SurfaceId("synthetic:support-plane".into());
    let offset_plane = SurfaceId("synthetic:offset-plane".into());
    let plane = |id, origin| Surface {
        id,
        geometry: SurfaceGeometry::Plane {
            origin,
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        source_object: None,
    };
    ir.model.surfaces.extend([
        plane(support_plane.clone(), Point3::new(10.0, 20.0, 30.0)),
        plane(offset_plane.clone(), Point3::new(10.0, 20.0, 35.0)),
    ]);
    assert_eq!(
        crate::decode::constant_surface_offset_between(&ir, &support_plane, &offset_plane, 0),
        Some(5.0)
    );
    let SurfaceGeometry::Plane { origin, .. } = &mut ir.model.surfaces[3].geometry else {
        unreachable!()
    };
    origin.x += 1.0;
    assert!(
        crate::decode::constant_surface_offset_between(&ir, &support_plane, &offset_plane, 0)
            .is_none()
    );
}

#[test]
fn blend_contact_matches_concentric_blend_carriers() {
    use cadmpeg_ir::geometry::{BlendSupport, ProceduralSurface, Surface};
    use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let first = SurfaceId("synthetic:first".into());
    let second = SurfaceId("synthetic:second".into());
    let first_offset = SurfaceId("synthetic:first-offset".into());
    let second_offset = SurfaceId("synthetic:second-offset".into());
    let plane = |id, origin, normal, u_axis| Surface {
        id,
        geometry: SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        },
        source_object: None,
    };
    ir.model.surfaces.extend([
        plane(
            first.clone(),
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        plane(
            second.clone(),
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        plane(
            first_offset.clone(),
            Point3::new(3.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
        plane(
            second_offset.clone(),
            Point3::new(0.0, 3.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ),
    ]);

    let spine = CurveId("synthetic:shared-spine".into());
    let inner = SurfaceId("synthetic:inner-blend".into());
    let outer = SurfaceId("synthetic:outer-blend".into());
    for (surface, supports, radius) in [
        (inner.clone(), [first, second], 0.7),
        (outer.clone(), [first_offset, second_offset], 3.7),
    ] {
        let construction = ProceduralSurfaceId(format!("{}:construction", surface.0));
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
            },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: construction,
            surface,
            definition: ProceduralSurfaceDefinition::Blend {
                supports: supports.map(|surface| {
                    Some(BlendSupport {
                        surface,
                        reversed: false,
                    })
                }),
                spine: Some(spine.clone()),
                radius: BlendRadiusLaw::Constant {
                    signed_radius: radius,
                },
                cross_section: BlendCrossSection::Circular,
                native: None,
            },
            cache_fit_tolerance: None,
        });
    }

    assert_eq!(
        crate::decode::constant_surface_offset_between(&ir, &inner, &outer, 0),
        Some(3.0)
    );
    let outer_definition = ir
        .model
        .procedural_surfaces
        .iter_mut()
        .find(|candidate| candidate.surface == outer)
        .unwrap();
    let ProceduralSurfaceDefinition::Blend { supports, .. } = &mut outer_definition.definition
    else {
        unreachable!()
    };
    supports[0].as_mut().unwrap().reversed = true;
    assert!(crate::decode::constant_surface_offset_between(&ir, &inner, &outer, 0).is_none());
}

#[test]
fn closest_spine_parameter_inverts_periodic_analytic_curves() {
    use cadmpeg_ir::geometry::Curve;
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::math::Point3;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let ellipse = CurveId("synthetic:ellipse-spine".into());
    let geometry = CurveGeometry::Ellipse {
        center: Point3::new(2.0, 3.0, 4.0),
        axis: Vector3::new(0.0, 1.0, 0.0),
        major_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 12.0,
        minor_radius: 5.0,
    };
    let parameter = 1.2;
    let mut point = cadmpeg_ir::eval::curve_point(&geometry, parameter).unwrap();
    point.y += 3.0;
    ir.model.curves.push(Curve {
        id: ellipse.clone(),
        geometry,
        source_object: None,
    });

    let first = crate::decode::closest_spine_parameter(&ir, &ellipse, point, None).unwrap();
    let continued = crate::decode::closest_spine_parameter(
        &ir,
        &ellipse,
        point,
        Some(parameter + std::f64::consts::TAU),
    )
    .unwrap();

    assert!((first - parameter).abs() < 1.0e-8, "{first}");
    assert!(
        (continued - parameter - std::f64::consts::TAU).abs() < 1.0e-8,
        "{continued}"
    );
}

#[test]
fn rolling_ball_blend_parameters_invert_the_canal_surface_law() {
    use cadmpeg_ir::geometry::{
        BlendSupport, Curve, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
        ProceduralCurveDefinition, ProceduralSurface, Surface,
    };
    use cadmpeg_ir::ids::{
        CurveId, EdgeId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, VertexId,
    };
    use cadmpeg_ir::topology::Edge;

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let first = SurfaceId("synthetic:first-plane".into());
    let second = SurfaceId("synthetic:second-plane".into());
    ir.model.surfaces.extend([
        Surface {
            id: first.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: second.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    let first_spine_side = SurfaceId("synthetic:first-spine-side".into());
    let second_spine_side = SurfaceId("synthetic:second-spine-side".into());
    ir.model.surfaces.extend([
        Surface {
            id: first_spine_side.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0),
                normal: Vector3::new(1.0, 0.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
        Surface {
            id: second_spine_side.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
                normal: Vector3::new(0.0, 1.0, 0.0),
                u_axis: Vector3::new(0.0, 0.0, 1.0),
            },
            source_object: None,
        },
    ]);
    let spine = CurveId("synthetic:spine".into());
    ir.model.curves.push(Curve {
        id: spine.clone(),
        geometry: CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let surface = SurfaceId("synthetic:blend".into());
    let construction = ProceduralSurfaceId("synthetic:blend-construction".into());
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: construction,
        surface: surface.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface: first.clone(),
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: second.clone(),
                    reversed: false,
                }),
            ],
            spine: Some(spine.clone()),
            radius: BlendRadiusLaw::Constant { signed_radius: 2.0 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
    });
    let expected = Point2::new(8.0, 0.35);
    let point = crate::decode::blend_surface_point(&ir, &surface, expected.u, expected.v).unwrap();

    assert_eq!(
        crate::decode::blend_spine_cache_fit_tolerance(&ir, &surface, 0.25),
        0.25
    );
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("synthetic:spine-construction".into()),
        curve: spine.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(first_spine_side),
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, -2.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(second_spine_side),
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, 2.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                ],
                parameter_range: [0.0, 10.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: Some(0.75),
    });
    assert_eq!(
        crate::decode::blend_spine_cache_fit_tolerance(&ir, &surface, 0.25),
        1.0
    );

    let actual = crate::decode::blend_surface_parameters(&ir, &surface, point, None).unwrap();

    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);
    let continued = crate::decode::blend_surface_parameters_for_fit(
        &ir,
        &surface,
        point,
        Some(Point2::new(expected.u + 0.1, expected.v - 0.05)),
        1.0e-8,
    )
    .unwrap();
    assert!((continued.u - expected.u).abs() < 1.0e-8);
    assert!((continued.v - expected.v).abs() < 1.0e-8);

    let boundary_curve = CurveId("synthetic:blend-boundary-curve".into());
    ir.model.procedural_curves.push(ProceduralCurve {
        id: ProceduralCurveId("synthetic:blend-boundary".into()),
        curve: boundary_curve.clone(),
        definition: ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext {
                sides: [
                    IntcurveSupportSide {
                        surface: Some(first.clone()),
                        pcurve: Some(PcurveGeometry::Line {
                            origin: Point2::new(0.0, -2.0),
                            direction: Point2::new(1.0, 0.0),
                        }),
                    },
                    IntcurveSupportSide {
                        surface: Some(surface.clone()),
                        pcurve: None,
                    },
                ],
                parameter_range: [0.0, 1.0],
                discontinuities: [Vec::new(), Vec::new(), Vec::new()],
            },
            discontinuity_flag: false,
        },
        cache_fit_tolerance: None,
    });
    ir.model.edges.push(Edge {
        id: EdgeId("synthetic:blend-boundary-edge".into()),
        curve: Some(boundary_curve),
        start: VertexId("synthetic:blend-boundary-start".into()),
        end: VertexId("synthetic:blend-boundary-end".into()),
        param_range: Some([0.0, 1.0]),
        tolerance: Some(1.0e-8),
    });
    crate::decode::complete_intersection_pcurves_from_opposite_charts(&mut ir);
    let ProceduralCurveDefinition::Intersection { context, .. } =
        &ir.model.procedural_curves.last().unwrap().definition
    else {
        unreachable!()
    };
    let PcurveGeometry::Nurbs { control_points, .. } = context.sides[1].pcurve.as_ref().unwrap()
    else {
        unreachable!()
    };
    assert_eq!(control_points.first(), Some(&Point2::new(0.0, 0.0)));
    assert_eq!(control_points.last(), Some(&Point2::new(1.0, 0.0)));
    assert_eq!(
        crate::decode::blend_boundary_parameter_from_support_spine(
            &ir,
            &surface,
            &first,
            cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
            None,
            1.0e-8,
        ),
        Some(Point2::new(0.0, 0.0))
    );
    ir.model
        .procedural_curves
        .iter_mut()
        .find(|procedural| procedural.curve == spine)
        .unwrap()
        .definition = ProceduralCurveDefinition::Unknown { record: None };
    assert_eq!(
        crate::decode::blend_boundary_parameter_from_support_spine(
            &ir,
            &surface,
            &first,
            cadmpeg_ir::math::Point3::new(0.0, 2.0, 0.0),
            None,
            1.0e-8,
        ),
        Some(Point2::new(0.0, 0.0))
    );

    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine)
        .unwrap()
        .geometry = CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 10.0, 10.0],
        control_points: vec![
            cadmpeg_ir::math::Point3::new(2.0, 2.0, 0.0),
            cadmpeg_ir::math::Point3::new(2.0, 2.0, 10.0),
        ],
        weights: None,
        periodic: false,
    });
    let coarse = crate::decode::coarse_blend_surface_parameters(&ir, &surface, point, 0).unwrap();
    let coarse_point =
        crate::decode::blend_surface_point(&ir, &surface, coarse.u, coarse.v).unwrap();
    assert!(
        ((coarse_point.x - point.x).powi(2)
            + (coarse_point.y - point.y).powi(2)
            + (coarse_point.z - point.z).powi(2))
        .sqrt()
            < 1.0
    );

    let refined = crate::decode::refine_blend_surface_parameters(
        &ir,
        &surface,
        point,
        Point2::new(expected.u + 0.5, expected.v + 0.1),
        0,
    )
    .unwrap();
    let refined_point =
        crate::decode::blend_surface_point(&ir, &surface, refined.u, refined.v).unwrap();
    let refined_error = ((refined_point.x - point.x).powi(2)
        + (refined_point.y - point.y).powi(2)
        + (refined_point.z - point.z).powi(2))
    .sqrt();
    assert!(refined_error < 1.0e-9);

    let third = SurfaceId("synthetic:third-plane".into());
    ir.model.surfaces.push(Surface {
        id: third.clone(),
        geometry: SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 8.0, 0.0),
            normal: Vector3::new(0.0, 1.0, 0.0),
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let outer_spine = CurveId("synthetic:outer-spine".into());
    ir.model.curves.push(Curve {
        id: outer_spine.clone(),
        geometry: CurveGeometry::Line {
            origin: cadmpeg_ir::math::Point3::new(4.0, 6.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    let outer = SurfaceId("synthetic:outer-blend".into());
    let outer_construction = ProceduralSurfaceId("synthetic:outer-blend-construction".into());
    ir.model.surfaces.push(Surface {
        id: outer.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: outer_construction.clone(),
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: outer_construction,
        surface: outer.clone(),
        definition: ProceduralSurfaceDefinition::Blend {
            supports: [
                Some(BlendSupport {
                    surface,
                    reversed: false,
                }),
                Some(BlendSupport {
                    surface: third,
                    reversed: false,
                }),
            ],
            spine: Some(outer_spine),
            radius: BlendRadiusLaw::Constant { signed_radius: 1.5 },
            cross_section: BlendCrossSection::Circular,
            native: None,
        },
        cache_fit_tolerance: None,
    });
    let expected = Point2::new(4.0, 0.2);
    let point = crate::decode::blend_surface_point(&ir, &outer, expected.u, expected.v).unwrap();
    let actual = crate::decode::blend_surface_parameters(&ir, &outer, point, None).unwrap();
    assert!((actual.u - expected.u).abs() < 1.0e-8);
    assert!((actual.v - expected.v).abs() < 1.0e-8);

    let outer_definition = ir
        .model
        .procedural_surfaces
        .iter_mut()
        .find(|candidate| candidate.surface == outer)
        .unwrap();
    let ProceduralSurfaceDefinition::Blend { supports, .. } = &mut outer_definition.definition
    else {
        panic!("blend definition");
    };
    supports[0].as_mut().unwrap().surface = outer.clone();
    assert!(crate::decode::blend_surface_point(&ir, &outer, expected.u, expected.v).is_none());
}

#[test]
fn decode_emits_both_intersection_support_pcurves() {
    let stream = two_support_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    assert!(context.sides[0].surface.is_some());
    assert!(context.sides[0].pcurve.is_some());
    assert!(context.sides[1].surface.is_some());
    assert!(context.sides[1].pcurve.is_some());
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_intersection_second_support_through_blend_bound() {
    let stream = blend_bound_charted_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidBlendBoundRecord>("parasolid_blend_bound_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].header_references, [1; 5]);
    assert!(records[0].sense);
    assert_eq!(records[0].boundary_index, 0);
    assert_eq!(records[0].blend_surface_xmt, 13);
    assert!(!records[0].escaped);

    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { context, .. } =
        &result.ir.model.procedural_curves[0].definition
    else {
        panic!("typed intersection");
    };
    let second = context.sides[1].surface.as_ref().expect("bridged support");
    assert_ne!(context.sides[0].surface.as_ref(), Some(second));
    assert!(context.sides[1].pcurve.is_some());
}

#[test]
fn decode_emits_inline_descriptor_intersection_witnesses() {
    let stream = inline_descriptor_intersection_curve_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir.model.procedural_curves[0].definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { .. }
    ));
    assert!(matches!(
        result
            .ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == result.ir.model.procedural_curves[0].curve)
            .expect("intersection curve")
            .geometry,
        CurveGeometry::Nurbs(_)
    ));
}

#[test]
fn decode_emits_topology_when_record_xmt_uses_extended_encoding() {
    let stream = large_xmt_headers(&topology_partition_stream());
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_maps_parasolid_tolerance_sentinel_to_none() {
    let stream = topology_with_missing_tolerances();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.vertices[0].tolerance, None);
    assert_eq!(result.ir.model.edges[0].tolerance, None);
    assert_eq!(result.ir.model.faces[0].tolerance, None);
}

#[test]
fn decode_dual_writes_inline_entity_metadata_to_annotations() {
    let mut cur = Cursor::new(topology_part_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let ir = &result.ir;
    let annotations = &result.source_fidelity.annotations;

    macro_rules! assert_arena_annotations {
        ($arena:expr) => {
            for entity in $arena {
                let provenance = annotations
                    .provenance
                    .get(&entity.id.to_string())
                    .expect("annotation provenance");
                assert!(annotations.streams[provenance.stream as usize].starts_with("nx:"));
                assert!(provenance.tag.is_some());
            }
        };
    }

    assert_arena_annotations!(&ir.model.bodies);
    assert_arena_annotations!(&ir.model.regions);
    assert_arena_annotations!(&ir.model.shells);
    assert_arena_annotations!(&ir.model.faces);
    assert_arena_annotations!(&ir.model.loops);
    assert_arena_annotations!(&ir.model.coedges);
    assert_arena_annotations!(&ir.model.edges);
    assert_arena_annotations!(&ir.model.vertices);
    assert_arena_annotations!(&ir.model.points);
    assert_arena_annotations!(&ir.model.surfaces);
    assert_arena_annotations!(&ir.model.curves);
    let unknowns = ir.native_unknowns("nx").unwrap();
    assert_arena_annotations!(&unknowns);

    let point_note = &annotations.exactness[&ir.model.points[0].id.to_string()];
    assert_eq!(point_note.entity, Exactness::ByteExact);
    assert_eq!(point_note.fields["position"], Exactness::Derived);
    let surface_note = &annotations.exactness[&ir.model.surfaces[0].id.to_string()];
    assert_eq!(surface_note.fields["geometry"], Exactness::Derived);
    let curve_note = &annotations.exactness[&ir.model.curves[0].id.to_string()];
    assert_eq!(curve_note.fields["geometry"], Exactness::Derived);
    for id in [
        ir.model.vertices[0].id.to_string(),
        ir.model.edges[0].id.to_string(),
        ir.model.faces[0].id.to_string(),
    ] {
        assert_eq!(
            annotations.exactness[&id].fields["tolerance"],
            Exactness::Derived
        );
    }
}

#[test]
fn decode_transfers_bspline_surface_and_curve() {
    let stream = bspline_partition_stream();
    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let surface = result
        .ir
        .model
        .surfaces
        .iter()
        .find_map(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(surface) => Some(surface),
            _ => None,
        })
        .expect("B-spline surface");
    assert_eq!(surface.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_points.len(), 4);
    assert!((surface.control_points[1].y - 20.0).abs() < 1e-9);
    let curve = result
        .ir
        .model
        .curves
        .iter()
        .find_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(curve) => Some(curve),
            _ => None,
        })
        .expect("B-spline curve");
    assert_eq!(curve.knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(curve.control_points.len(), 2);
    assert!((curve.control_points[1].x - 20.0).abs() < 1e-9);
}

#[test]
fn nurbs_decodes_extended_xmt_arrays_payload_and_long_surface_descriptor() {
    let surfaces = crate::nurbs::surfaces(&extended_bspline_surface_stream());
    assert_eq!(surfaces.len(), 1);
    let SurfaceGeometry::Nurbs(surface) = &surfaces[0].geometry else {
        panic!("expected NURBS surface");
    };
    assert_eq!(surface.u_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.v_knots, vec![0.0, 0.0, 1.0, 1.0]);
    assert_eq!(surface.control_points.len(), 4);
    assert_eq!(surface.control_points[3].y, 20.0);
}

#[test]
fn nurbs_decodes_escaped_curve_descriptor_and_payload_count() {
    let mut stream = bspline_partition_stream();
    let descriptor = stream
        .windows(4)
        .position(|window| window == [0, 136, 0, 40])
        .expect("curve descriptor");
    stream.insert(descriptor + 2, 0xff);
    let payload = stream
        .windows(4)
        .position(|window| window == [0, 135, 0, 41])
        .expect("curve payload");
    stream.insert(payload + 2, 0xff);
    stream.insert(payload + 10, 0xff);

    let curves = crate::nurbs::curves(&stream);
    assert_eq!(curves.len(), 1);
    let CurveGeometry::Nurbs(curve) = &curves[0].geometry else {
        panic!("expected NURBS curve");
    };
    assert_eq!(curve.control_points.len(), 2);
    assert_eq!(curve.control_points[1].x, 20.0);
}

#[test]
fn decode_replaces_partition_bspline_surface_wrapper_from_deltas() {
    let partition = bspline_surface_replacement_partition_stream();
    let deltas = deltas_bspline_surface_wrapper_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.surfaces.iter().any(|surface| matches!(
        &surface.geometry,
        SurfaceGeometry::Nurbs(nurbs)
            if nurbs.control_points.iter().any(|point| point.y == 30.0)
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_replaces_partition_bspline_curve_wrapper_from_deltas() {
    let partition = bspline_curve_replacement_partition_stream();
    let deltas = deltas_bspline_curve_wrapper_stream();
    let file = prt_with_streams(&[&partition, &deltas]);
    let mut cur = Cursor::new(file);
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(result.ir.model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        CurveGeometry::Nurbs(nurbs)
            if nurbs.control_points.iter().any(|point| point.y == 10.0)
    )));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_trimmed_edge_to_its_basis_curve_and_range() {
    let mut cur = Cursor::new(prt_with_partition(&trimmed_topology_partition_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    assert_eq!(edge.curve.as_ref(), Some(&result.ir.model.curves[0].id));
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    let records = result
        .ir
        .native
        .namespace("nx")
        .unwrap()
        .arena_as::<crate::native::ParasolidTrimmedCurveRecord>("parasolid_trimmed_curve_records")
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].basis_xmt, 9);
    assert_eq!(records[0].points, [[0.0; 3]; 2]);
    assert_eq!(records[0].parameters, [0.000_25, 0.000_75]);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_uses_partner_fin_vertex_for_edge_endpoint() {
    let mut cur = Cursor::new(prt_with_partition(
        &partnered_trimmed_topology_partition_stream(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    assert_ne!(edge.start, edge.end);
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    assert_eq!(result.ir.model.coedges.len(), 2);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_forward_trimmed_curve_chain() {
    let mut cur = Cursor::new(prt_with_partition(&forward_trimmed_curve_chain_stream()));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    assert_eq!(edge.curve.as_ref(), Some(&result.ir.model.curves[0].id));
    assert_eq!(edge.param_range, Some([0.25, 0.75]));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_retains_a_curve_when_its_trim_range_misses_edge_vertices() {
    let mut cur = Cursor::new(prt_with_partition(
        &mismatched_trimmed_topology_partition_stream(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let edge = result.ir.model.edges.first().expect("edge");
    let carrier = edge
        .curve
        .as_ref()
        .and_then(|id| result.ir.model.curves.iter().find(|curve| curve.id == *id))
        .expect("edge carrier");
    assert!(matches!(carrier.geometry, CurveGeometry::Line { .. }));
    assert_eq!(edge.param_range, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_omits_overflowing_line_trim_range() {
    let mut stream = trimmed_topology_partition_stream();
    let trim = stream
        .windows(4)
        .position(|window| window == [0, 133, 0, 12])
        .expect("trimmed curve");
    put_f64(&mut stream, trim + 69, f64::MAX);

    let mut cur = Cursor::new(prt_with_partition(&stream));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.edges[0].param_range, None);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_resolves_extended_xmt_reference_inside_edge_record() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_edge_curve_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
}

#[test]
fn decode_tracks_extended_face_reference_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_face_attribute_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.faces[0].tolerance, Some(0.2));
    assert_eq!(
        result.ir.model.faces[0].surface,
        result.ir.model.surfaces[0].id
    );
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_tracks_extended_edge_reference_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_edge_attribute_reference(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.edges[0].tolerance, Some(0.3));
    assert_eq!(
        result.ir.model.edges[0].curve.as_ref(),
        Some(&result.ir.model.curves[0].id)
    );
}

#[test]
fn decode_tracks_all_extended_topology_reference_shifts() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_extended_internal_topology_references(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(result.ir.model.shells.len(), 1);
    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.loops.len(), 1);
    assert_eq!(result.ir.model.coedges.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert_eq!(result.ir.model.vertices.len(), 1);
    assert_eq!(result.ir.model.vertices[0].tolerance, Some(0.1));
    assert_eq!(result.ir.model.points[0].position.x, 10.0);
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_tracks_fully_extended_geometry_header_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_fully_extended_geometry_headers(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.faces.len(), 1);
    assert_eq!(result.ir.model.edges.len(), 1);
    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
    assert!(matches!(
        result.ir.model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
}

#[test]
fn decode_tracks_geometry_envelope_escape_shift() {
    let mut cur = Cursor::new(prt_with_partition(
        &topology_with_escaped_geometry_envelopes(),
    ));
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert!(matches!(
        result.ir.model.surfaces[0].geometry,
        SurfaceGeometry::Plane { .. }
    ));
    assert!(matches!(
        result.ir.model.curves[0].geometry,
        CurveGeometry::Line { .. }
    ));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn cylinder_gate_rejects_denormal_radius() {
    // A coincidental byte alignment can present a unit axis and a model-scale
    // origin alongside a denormal (near-zero) double at the radius slot; the radius
    // floor must reject it rather than emit a fabricated zero-radius cylinder.
    let mut cy = record(0x33, 99);
    put_vec3(&mut cy, 19, [0.003_175, 0.0, 0.0]);
    put_vec3(&mut cy, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cy, 67, f64::from_bits(1)); // smallest positive subnormal
    put_vec3(&mut cy, 75, [1.0, 0.0, 0.0]);
    assert!(crate::geometry::surfaces(&cy).is_empty());
}

#[test]
fn graph_owned_analytic_geometry_has_no_scanner_magnitude_limit() {
    let mut cylinder = record(0x33, 99);
    put_vec3(&mut cylinder, 19, [1_001.0, 0.0, 0.0]);
    put_vec3(&mut cylinder, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cylinder, 67, f64::from_bits(1));
    put_vec3(&mut cylinder, 75, [1.0, 0.0, 0.0]);

    assert!(crate::geometry::surfaces(&cylinder).is_empty());
    let geometry =
        crate::geometry::decode_surface_record(&cylinder, 0x33, 0).expect("graph-owned cylinder");
    let SurfaceGeometry::Cylinder { origin, radius, .. } = geometry else {
        panic!("cylinder")
    };
    assert_eq!(origin.x, 1_001_000.0);
    assert_eq!(radius, f64::from_bits(1) * 1000.0);

    put_f64(&mut cylinder, 67, f64::INFINITY);
    assert!(crate::geometry::decode_surface_record(&cylinder, 0x33, 0).is_none());
}

#[test]
fn ellipse_requires_ordered_serialized_radii() {
    let mut ellipse = record(0x20, 107);
    put_vec3(&mut ellipse, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut ellipse, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut ellipse, 67, [1.0, 0.0, 0.0]);
    put_f64(&mut ellipse, 91, 0.01);
    put_f64(&mut ellipse, 99, 0.01 + 5.0e-10);

    assert!(crate::geometry::curves(&ellipse).is_empty());
    assert!(crate::geometry::decode_curve_record(&ellipse, 0x20, 0).is_none());

    put_f64(&mut ellipse, 99, 0.01);
    assert_eq!(crate::geometry::curves(&ellipse).len(), 1);
}

#[test]
fn graph_owned_point_has_no_scanner_magnitude_limit() {
    let mut stream = topology_partition_stream();
    let point = stream
        .windows(4)
        .position(|window| window == [0, 29, 0, 11])
        .expect("point record");
    put_vec3(&mut stream, point + 16, [1_001.0, f64::from_bits(1), 0.0]);

    assert!(crate::geometry::points(&stream).is_empty());
    let graph = crate::topology::Graph::parse(&stream);
    assert_eq!(
        graph
            .get(29, 11)
            .and_then(crate::topology::Node::point_position),
        Some(cadmpeg_ir::math::Point3::new(
            1_001_000.0,
            f64::from_bits(1) * 1000.0,
            0.0,
        ))
    );

    put_vec3(&mut stream, point + 16, [f64::INFINITY, 0.0, 0.0]);
    assert!(crate::topology::Graph::parse(&stream).get(29, 11).is_none());
}

#[test]
fn decoded_tolerance_has_no_model_magnitude_limit() {
    assert_eq!(crate::decode::decoded_tolerance(1_001.0), Some(1_001_000.0));
    assert_eq!(crate::decode::decoded_tolerance(0.0), None);
    assert_eq!(crate::decode::decoded_tolerance(f64::INFINITY), None);
    assert_eq!(crate::decode::decoded_tolerance(f64::MAX), None);
}

#[test]
fn analytic_frame_gate_rejects_nonorthogonal_reference_direction() {
    let mut plane = record(0x32, 91);
    put_vec3(&mut plane, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut plane, 43, [0.0, 0.0, 1.0]);
    put_vec3(&mut plane, 67, [0.0, 0.0, 1.0]);
    assert!(crate::geometry::surfaces(&plane).is_empty());

    put_vec3(&mut plane, 67, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::surfaces(&plane).len(), 1);
}

#[test]
fn cone_gate_rejects_nonfinite_or_degenerate_half_angle() {
    let mut cone = record(0x34, 115);
    put_vec3(&mut cone, 19, [0.0, 0.0, 0.0]);
    put_vec3(&mut cone, 43, [0.0, 0.0, 1.0]);
    put_f64(&mut cone, 67, 0.0);
    put_f64(&mut cone, 75, std::f64::consts::FRAC_1_SQRT_2);
    put_f64(&mut cone, 83, std::f64::consts::FRAC_1_SQRT_2);
    put_vec3(&mut cone, 91, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::surfaces(&cone).len(), 1);

    for (sine, cosine) in [(f64::NAN, 1.0), (0.0, 1.0), (1.0, 0.0)] {
        put_f64(&mut cone, 75, sine);
        put_f64(&mut cone, 83, cosine);
        assert!(crate::geometry::surfaces(&cone).is_empty());
    }
}

#[test]
fn analytic_scanners_include_extended_reference_shifts_in_record_ownership() {
    let mut surfaces = vec![0; 182];
    surfaces[1] = 0x32;
    put_vec3(&mut surfaces, 21, [0.0, 0.0, 0.0]);
    put_vec3(&mut surfaces, 45, [0.0, 0.0, 1.0]);
    put_vec3(&mut surfaces, 69, [1.0, 0.0, 0.0]);
    surfaces[91] = 0;
    surfaces[92] = 0x32;
    put_vec3(&mut surfaces, 110, [0.0, 0.0, 0.0]);
    put_vec3(&mut surfaces, 134, [0.0, 0.0, 1.0]);
    put_vec3(&mut surfaces, 158, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::surfaces(&surfaces).len(), 1);

    let mut curves = vec![0; 134];
    curves[1] = 0x1e;
    put_vec3(&mut curves, 21, [0.0, 0.0, 0.0]);
    put_vec3(&mut curves, 45, [1.0, 0.0, 0.0]);
    curves[67] = 0;
    curves[68] = 0x1e;
    put_vec3(&mut curves, 86, [0.0, 0.0, 0.0]);
    put_vec3(&mut curves, 110, [1.0, 0.0, 0.0]);
    assert_eq!(crate::geometry::curves(&curves).len(), 1);
}

#[test]
fn analytic_record_ownership_is_shared_across_carrier_families() {
    let mut stream = vec![0; 158];
    stream[1] = 0x1e;
    put_vec3(&mut stream, 21, [0.0, 0.0, 0.0]);
    put_vec3(&mut stream, 45, [1.0, 0.0, 0.0]);

    stream[67] = 0;
    stream[68] = 0x32;
    put_vec3(&mut stream, 86, [0.0, 0.0, 0.0]);
    put_vec3(&mut stream, 110, [0.0, 0.0, 1.0]);
    put_vec3(&mut stream, 134, [1.0, 0.0, 0.0]);

    assert_eq!(crate::geometry::curves(&stream).len(), 1);
    assert!(crate::geometry::surfaces(&stream).is_empty());
    assert!(crate::geometry::points(&stream).is_empty());
}

#[test]
fn decode_assembly_reports_external_dependency() {
    let mut cur = Cursor::new(assembly_prt());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    assert!(!result.report.geometry_transferred);
    assert!(result
        .report
        .losses
        .iter()
        .any(|l| l.message.contains("assembly")));
}

#[test]
fn assembly_metadata_lists_external_child_paths() {
    let mut cur = Cursor::new(assembly_with_external_paths());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let attrs = &result.ir.source.expect("source").attributes;
    assert_eq!(
        attrs.get("external_reference.0").map(String::as_str),
        Some("child.prt")
    );
    assert_eq!(
        attrs.get("external_reference.1").map(String::as_str),
        Some("nested/b.prt")
    );
    let references = result
        .ir
        .native
        .namespace("nx")
        .expect("NX native namespace")
        .arena_as::<crate::native::ExternalReference>("external_references")
        .expect("typed external references");
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].ordinal, 0);
    assert_eq!(references[0].path, "child.prt");
    assert_eq!(references[1].ordinal, 1);
    assert_eq!(references[1].path, "nested/b.prt");
    assert!(references[0].source_offset < references[1].source_offset);
}

#[test]
fn external_reference_string_table_is_end_anchored() {
    let table = b"prefix\x01\x02\x00\x00\x00\x09\x00child.prt\x0c\x00nested/b.prt";
    let (_, strings) = crate::container::parse_extref_string_table(table).expect("string table");
    assert_eq!(
        strings
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>(),
        ["child.prt", "nested/b.prt"]
    );

    let mut trailed = table.to_vec();
    trailed.push(0);
    assert!(crate::container::parse_extref_string_table(&trailed).is_none());
    assert!(crate::container::parse_extref_string_table(b"\x01\xff\xff\xff\xff").is_none());
}

#[test]
fn external_reference_record_slots_resolve_atomically_in_the_same_stream() {
    use crate::native::{
        external_reference_record_children, external_reference_record_string_uses,
        ExternalReference, ExternalReferenceRecord,
    };

    let references = (0..4)
        .map(|ordinal| ExternalReference {
            id: format!("reference#{ordinal}"),
            ordinal,
            path: format!("value-{ordinal}"),
            source_entry: "stream".into(),
            source_offset: 100 + u64::from(ordinal),
        })
        .collect::<Vec<_>>();
    let record = ExternalReferenceRecord {
        id: "record#7".into(),
        record_id: 7,
        declared_count: 2,
        id_slots: [0, 3, 1, 2],
        handles: vec![10, 20],
        closing_duplicate: true,
        prefix_byte_len: 40,
        tail_byte_len: 5,
        source_entry: "stream".into(),
        source_offset: 20,
    };
    let uses = external_reference_record_string_uses(&[record.clone()], &references);
    assert_eq!(uses.len(), 4);
    assert_eq!(uses[0].id, "nx:external-reference:record-string-use#7-0");
    assert_eq!(
        uses.iter().map(|use_| use_.slot).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    assert_eq!(
        uses.iter()
            .map(|use_| use_.string_index)
            .collect::<Vec<_>>(),
        [0, 3, 1, 2]
    );
    assert_eq!(uses[1].external_reference, "reference#3");
    assert_eq!(uses[1].source_offset, 31);
    let mut child_references = references.clone();
    child_references[0].path = "child.prt".into();
    let child_uses = external_reference_record_string_uses(&[record.clone()], &child_references);
    let children =
        external_reference_record_children(&[record.clone()], &child_references, &child_uses);
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].external_record, record.id);
    assert_eq!(children[0].name_reference, "reference#0");
    assert_eq!(children[0].directory_reference, "reference#1");
    assert!(external_reference_record_children(&[record.clone()], &references, &uses).is_empty());

    let mut out_of_range = record.clone();
    out_of_range.id_slots[2] = 4;
    assert!(external_reference_record_string_uses(&[out_of_range], &references).is_empty());
    let mut duplicate = references.clone();
    duplicate.push(references[0].clone());
    assert!(external_reference_record_string_uses(&[record], &duplicate).is_empty());
}

#[test]
fn external_reference_record_parser_requires_sorted_doubled_handle_set() {
    let mut payload = b"EXTREFSTREAM".to_vec();
    payload.extend_from_slice(&3u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.push(0);
    payload.extend_from_slice(&6u32.to_le_bytes());
    payload.extend_from_slice(&41u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(payload.len(), 41);
    payload.extend_from_slice(&[1, 0, 0, 0]);
    payload.extend_from_slice(&2u16.to_be_bytes());
    payload.push(1);
    for value in [8u32, 11, 12, 4] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend_from_slice(&[1, 4]);
    for handle in [0x1020_3040u32, 0x2030_4050, 0x2030_4050] {
        payload.push(0xe0);
        payload.extend_from_slice(&handle.to_be_bytes());
    }
    payload.push(4);
    payload.extend_from_slice(b"\x01\x01\x00\x00\x00\x09\x00child.prt");

    let records = crate::container::parse_extref_records(&payload);
    let indexed = crate::container::parse_extref_record_index(&payload).expect("record index");
    assert_eq!(indexed.len(), 1);
    assert_eq!(indexed[0].record_id, 6);
    assert_eq!(indexed[0].offset, 41);
    assert_eq!(indexed[0].byte_len, 41);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].record_id, 6);
    assert_eq!(records[0].declared_count, 2);
    assert_eq!(records[0].id_slots, [8, 11, 12, 4]);
    assert_eq!(records[0].handles, [0x1020_3040, 0x2030_4050]);
    assert!(records[0].closing_duplicate);
    assert_eq!(records[0].tail_byte_len, 0);

    let duplicate = payload
        .windows(5)
        .rposition(|window| window == [0xe0, 0x20, 0x30, 0x40, 0x50])
        .expect("closing duplicate");
    payload[duplicate + 1] = 0x10;
    assert!(crate::container::parse_extref_records(&payload).is_empty());
    assert_eq!(
        crate::container::parse_extref_record_index(&payload)
            .expect("opaque indexed record")
            .len(),
        1
    );
}

#[test]
fn external_reference_empty_record_parser_requires_the_complete_form() {
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1]),
        Some(false)
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1, 1]),
        Some(true)
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0, 1, 0]),
        None
    );
    assert_eq!(
        crate::container::parse_extref_empty_record(&[1, 0, 0, 0, 0]),
        None
    );
}

#[test]
fn external_reference_tail_pairs_require_adjacent_complete_tokens() {
    let bytes = [
        0xff, 0xe0, 0x12, 0x34, 0x56, 0x78, 0xca, 0xbc, 0xde, 0xf0, 0xe0, 0x00, 0x00, 0x00, 0x01,
        0x00,
    ];
    assert_eq!(
        crate::container::parse_extref_reference_pairs(&bytes),
        vec![(1, 0x1234_5678, 0x0abc_def0)]
    );
    assert!(crate::container::parse_extref_reference_pairs(&bytes[10..]).is_empty());
}

#[test]
fn persistent_handle_identity_bridges_om_and_external_records() {
    let reference = crate::native::ObjectReference {
        id: "nx:test:reference#0".into(),
        record: "nx:test:om-record#0".into(),
        object_id: Some(1),
        ordinal: 0,
        kind: crate::native::ObjectReferenceKind::PersistentHandle,
        value: 0x1020_3040,
        target_record: None,
        source_entry: "om".into(),
        source_offset: 0,
    };
    let external = crate::native::ExternalReferenceRecord {
        id: "nx:test:external-record#6".into(),
        record_id: 6,
        declared_count: 1,
        id_slots: [0; 4],
        handles: vec![0x1020_3040],
        closing_duplicate: true,
        prefix_byte_len: 31,
        tail_byte_len: 0,
        source_entry: "external".into(),
        source_offset: 10,
    };
    let control = crate::native::DataBlockControlReference {
        id: "nx:test:control-reference#0".into(),
        data_block: "nx:test:control-block#0".into(),
        ordinal: 0,
        kind: crate::native::ObjectReferenceKind::PersistentHandle,
        value: 0x1020_3040,
        source_offset: 20,
    };

    let tail_pair = crate::native::ExternalReferenceTailReferencePair {
        id: "nx:test:tail-pair#0".into(),
        handle_set_record: external.id.clone(),
        ordinal: 0,
        persistent_handle: 0x5060_7080,
        tagged_reference: 7,
        source_offset: 30,
    };

    let handles =
        crate::native::persistent_handles(&[reference], &[control], &[external], &[tail_pair]);

    assert_eq!(handles.len(), 2);
    assert_eq!(handles[0].records, ["nx:test:om-record#0"]);
    assert_eq!(handles[0].occurrence_count, 2);
    assert_eq!(handles[0].data_blocks, ["nx:test:control-block#0"]);
    assert_eq!(handles[0].external_records, ["nx:test:external-record#6"]);
    assert_eq!(handles[0].external_occurrence_count, 2);
    assert_eq!(handles[1].value, 0x5060_7080);
    assert_eq!(handles[1].external_records, ["nx:test:external-record#6"]);
    assert_eq!(handles[1].external_occurrence_count, 1);
}

#[test]
fn nx_control_handle_pairs_require_maximal_runs_of_exactly_two() {
    let reference = |ordinal: u32, offset: u64| crate::native::DataBlockControlReference {
        id: format!("reference#{ordinal}"),
        data_block: "block#0".into(),
        ordinal,
        kind: crate::native::ObjectReferenceKind::PersistentHandle,
        value: ordinal + 100,
        source_offset: offset,
    };
    let references = [
        reference(0, 10),
        reference(1, 15),
        reference(2, 30),
        reference(3, 35),
        reference(4, 40),
    ];
    let pairs = crate::native::data_block_control_handle_pairs(&references);
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].id, "nx:om-data-block-control:handle-pair#10");
    assert_eq!(pairs[0].first_reference, "reference#0");
    assert_eq!(pairs[0].second_reference, "reference#1");
    assert_eq!(pairs[0].first_handle, 100);
    assert_eq!(pairs[0].second_handle, 101);
}

#[test]
fn container_reads_rmfastload_active_ids() {
    let container = container::scan_bytes(rmfastload_prt()).unwrap();
    let (entry, table) = container
        .rmfastload_object_id_table()
        .expect("RMFastLoad object-id table");
    assert_eq!(entry.name, "/Root/FastLoad/RMFastLoad");
    assert_eq!(table.registry_offset, 0);
    assert_eq!(table.count_offset, b"UGS::Solid::Topol".len());
    assert_eq!(table.raw_count, 50u32.to_le_bytes());
    assert_eq!(
        table
            .object_ids
            .iter()
            .map(|object_id| object_id.value)
            .collect::<Vec<_>>(),
        (1..=50).collect::<Vec<_>>()
    );
    assert_eq!(table.object_ids[0].offset, table.count_offset + 4);
    assert_eq!(table.object_ids[0].raw, 1u32.to_le_bytes());
    assert_eq!(table.object_ids[49].offset, table.count_offset + 4 + 49 * 4);
    assert_eq!(table.object_ids[49].raw, 50u32.to_le_bytes());
}

#[test]
fn native_retains_rmfastload_table_and_member_words() {
    let container = container::scan_bytes(rmfastload_prt()).unwrap();
    let entry_offset = container
        .entries
        .iter()
        .find(|entry| entry.name == "/Root/FastLoad/RMFastLoad")
        .and_then(|entry| entry.file_span)
        .expect("RMFastLoad span")
        .0;
    let (table, object_ids) =
        crate::native::rmfastload_object_id_table(&container).expect("native RMFastLoad table");

    assert_eq!(table.id, "nx:rmfastload:object-id-table#0");
    assert_eq!(table.members.len(), 50);
    assert_eq!(table.raw_count, 50u32.to_le_bytes());
    assert_eq!(table.registry_source_offset, entry_offset);
    assert_eq!(
        table.source_offset,
        entry_offset + b"UGS::Solid::Topol".len() as u64
    );
    assert_eq!(object_ids[0].table, table.id);
    assert_eq!(object_ids[0].value, 1);
    assert_eq!(object_ids[0].raw, 1u32.to_le_bytes());
    assert_eq!(object_ids[0].source_offset, table.source_offset + 4);
    assert_eq!(object_ids[49].ordinal, 49);
    assert_eq!(object_ids[49].value, 50);
    assert_eq!(object_ids[49].raw, 50u32.to_le_bytes());
    assert_eq!(table.members[49], object_ids[49].id);
}

#[test]
fn decode_selects_dominant_rmfastload_body() {
    let mut cur = Cursor::new(prt_with_two_bodies_and_rmfastload());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();
    let namespace = result.ir.native.namespace("nx").expect("NX namespace");
    let tables = namespace
        .arena_as::<crate::native::RmFastLoadObjectIdTable>("rmfastload_object_id_tables")
        .expect("RMFastLoad tables");
    let object_ids = namespace
        .arena_as::<crate::native::RmFastLoadObjectId>("rmfastload_object_ids")
        .expect("RMFastLoad object IDs");

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].members.len(), 50);
    assert_eq!(object_ids.len(), 50);
    assert_eq!(object_ids[0].value, 1_000);
    assert_eq!(object_ids[49].value, 1_049);
    assert!(result.ir.model.bodies[0].id.0.starts_with("nx:s0:"));
    assert_eq!(result.ir.model.faces.len(), 50);
    assert_eq!(result.ir.model.surfaces.len(), 50);
    assert!(result
        .ir
        .model
        .faces
        .iter()
        .all(|face| face.id.0.starts_with("nx:s0:")));
    assert!(result
        .ir
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.0.starts_with("nx:s0:")));
    assert_eq!(
        result
            .ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("active_body_selector"))
            .map(String::as_str),
        Some("rmfastload_object_id_membership")
    );
    let validation = cadmpeg_ir::validate::validate(&result.ir, Vec::new());
    assert!(
        validation.findings.is_empty(),
        "findings: {:?}",
        validation.findings
    );
}

#[test]
fn decode_retains_every_rmfastload_active_body() {
    let mut cur = Cursor::new(prt_with_two_active_bodies_and_rmfastload());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 2);
    assert_eq!(result.ir.model.faces.len(), 100);
    assert_eq!(
        result
            .ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get("rmfastload_active_body_count"))
            .map(String::as_str),
        Some("2")
    );
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("sub-body partition")));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_selects_active_shell_when_body_record_is_absent() {
    let mut cur = Cursor::new(prt_with_missing_active_body_record());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 1);
    assert!(result.ir.model.bodies[0].id.0.starts_with("nx:s0:"));
    assert_eq!(result.ir.model.faces.len(), 50);
    assert!(result
        .report
        .losses
        .iter()
        .all(|loss| !loss.message.contains("sub-body partition")));
    assert!(cadmpeg_ir::validate::validate(&result.ir, Vec::new()).is_ok());
}

#[test]
fn decode_keeps_bodies_when_rmfastload_overlap_is_weak() {
    let mut cur = Cursor::new(prt_with_weak_rmfastload_overlap());
    let result = NxCodec.decode(&mut cur, &DecodeOptions::default()).unwrap();

    assert_eq!(result.ir.model.bodies.len(), 2);
    assert!(result
        .ir
        .source
        .as_ref()
        .is_none_or(|source| !source.attributes.contains_key("active_body_selector")));
    assert!(result
        .report
        .losses
        .iter()
        .any(|loss| loss.message.contains("sub-body partition")));
}

#[test]
fn container_only_preserves_streams_without_geometry() {
    let mut cur = Cursor::new(single_part_prt());
    let opts = DecodeOptions {
        container_only: true,
    };
    let result = NxCodec.decode(&mut cur, &opts).unwrap();
    assert!(!result.report.geometry_transferred);
    assert!(result.report.container_only);
    assert_eq!(result.ir.native_unknowns("nx").unwrap().len(), 1);
    assert!(result.ir.model.points.is_empty());
}

#[test]
fn inspect_enumerates_streams_and_names_schema() {
    let mut cur = Cursor::new(single_part_prt());
    let summary = NxCodec.inspect(&mut cur).unwrap();
    assert_eq!(summary.format, "nx");
    assert_eq!(summary.container_kind, "splmsstr");
    assert!(summary.entries.iter().any(|e| e.role == "parasolid-stream"));
    assert!(summary.notes.iter().any(|n| n.contains("partition")));
}

#[test]
fn design_intent_losses_distinguish_native_and_sketch_gaps() {
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::features::{
        ConfigurationBodies, ConfigurationId, DesignConfiguration, Feature, FeatureDefinition,
        FeatureId,
    };

    let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
    for (ordinal, kind) in ["DELETE", "DELETE"].into_iter().enumerate() {
        ir.model.features.push(Feature {
            id: FeatureId(format!("test:feature#{ordinal}")),
            ordinal: ordinal as u64,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::Native {
                kind: kind.to_string(),
                parameters: Default::default(),
                properties: Default::default(),
            },
            native_ref: None,
        });
    }
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#sketch".into()),
        ordinal: 3,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Sketch {
            space: cadmpeg_ir::features::SketchSpace::Unresolved,
            sketch: None,
        },
        native_ref: None,
    });
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#incomplete-delete".into()),
        ordinal: 10,
        name: None,
        suppressed: Some(false),
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::DeleteBody {
            bodies: cadmpeg_ir::features::BodySelection::Unresolved,
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
        },
        native_ref: None,
    });
    for (ordinal, definition) in [
        FeatureDefinition::DatumPlaneUnresolved,
        FeatureDefinition::DatumCoordinateSystemUnresolved,
        FeatureDefinition::LoftUnresolved,
        FeatureDefinition::FreeformSurfaceUnresolved,
        FeatureDefinition::LoftUnresolved,
    ]
    .into_iter()
    .enumerate()
    {
        ir.model.features.push(Feature {
            id: FeatureId(format!("test:feature#unresolved-{ordinal}")),
            ordinal: ordinal as u64 + 4,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition,
            native_ref: None,
        });
    }
    ir.model.features.push(Feature {
        id: FeatureId("test:feature#incomplete-block".into()),
        ordinal: 9,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: FeatureDefinition::Block {
            dimensions: None,
            placement: None,
        },
        native_ref: None,
    });
    ir.model.configurations.extend([
        DesignConfiguration {
            id: ConfigurationId("test:configuration#0".into()),
            ordinal: 0,
            active: true,
            source_index: Some(0),
            name: "Model".into(),
            material: None,
            properties: Default::default(),
            bodies: ConfigurationBodies::Resolved(Vec::new()),
            native_ref: None,
        },
        DesignConfiguration {
            id: ConfigurationId("test:configuration#1".into()),
            ordinal: 1,
            active: false,
            source_index: Some(1),
            name: "Arrangement".into(),
            material: None,
            properties: Default::default(),
            bodies: ConfigurationBodies::Unresolved,
            native_ref: None,
        },
    ]);

    let mut losses = Vec::new();
    crate::decode::append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 6);
    assert_eq!(losses[0].category, LossCategory::Feature);
    assert!(losses[0].message.contains("9 NX feature history operation"));
    assert_eq!(losses[1].category, LossCategory::Feature);
    assert!(losses[1].message.contains("1 NX design configuration"));
    assert_eq!(losses[2].category, LossCategory::Feature);
    assert!(losses[2].message.contains("DELETE (2)"));
    assert_eq!(losses[3].category, LossCategory::Feature);
    assert!(losses[3].message.contains("datum coordinate system (1)"));
    assert!(losses[3].message.contains("datum plane (1)"));
    assert!(losses[3].message.contains("freeform surface (1)"));
    assert!(losses[3].message.contains("loft (2)"));
    assert_eq!(losses[4].category, LossCategory::Feature);
    assert!(losses[4].message.contains("block (1)"));
    assert!(losses[4].message.contains("delete body (1)"));
    assert!(losses[4].message.contains("sketch (1)"));
    assert_eq!(losses[5].category, LossCategory::Feature);
    assert!(losses[5].message.contains("1 NX sketch history feature"));
    assert!(losses[5].message.contains("1 have no neutral sketch graph"));

    let sketch_id = cadmpeg_ir::sketches::SketchId("test:sketch#0".into());
    ir.model.sketches.push(cadmpeg_ir::sketches::Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        u_axis: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        profiles: Vec::new(),
        native_ref: None,
    });
    ir.model.features[2].definition = FeatureDefinition::Sketch {
        space: cadmpeg_ir::features::SketchSpace::Planar,
        sketch: Some(sketch_id),
    };
    losses.clear();
    crate::decode::append_design_intent_losses(&ir, &mut losses);

    assert_eq!(losses.len(), 6);
    assert!(!losses[4].message.contains("sketch"));
    assert!(losses[5].message.contains("no sketch constraints"));
}

#[test]
fn extraction_uses_ug_part_bounds_and_all_standard_zlib_headers() {
    let part = zlib_compress_at_level(&partition_stream(), 6);
    assert_eq!(&part[..2], b"\x78\x9c");

    let mut decoy_stream = partition_stream();
    let schema = b"SCH_TEST_1_9999";
    let decoy = b"SCH_FAKE_1_9999";
    let pos = decoy_stream
        .windows(schema.len())
        .position(|w| w == schema)
        .unwrap();
    decoy_stream[pos..pos + schema.len()].copy_from_slice(decoy);
    let decoy = zlib_compress(&decoy_stream);

    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(0x06);
    file.extend_from_slice(&[0; 3 + 4 + 1 + 6 + 2]);
    file.extend_from_slice(b"HEADER");
    let entries = [
        (b"/Root/UG_PART/UG_PART".as_slice(), part.len()),
        (b"/Root/FastLoad/JT".as_slice(), decoy.len()),
    ];
    let directory_len: usize = entries.iter().map(|(name, _)| 4 + name.len() + 16).sum();
    let mut next_offset = file.len() + directory_len;
    for (name, size) in &entries {
        file.extend_from_slice(&(name.len() as u32).to_le_bytes());
        file.extend_from_slice(name);
        file.extend_from_slice(&(next_offset as u64).to_le_bytes());
        file.extend_from_slice(&(*size as u64).to_le_bytes());
        next_offset += size;
    }
    file.extend_from_slice(&part);
    file.extend_from_slice(&decoy);

    let streams = parasolid::extract_streams(&file);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].schema.as_deref(), Some("SCH_TEST_1_9999"));
}

/// Phase 0 golden serialized-output snapshots.
///
/// These freeze the NX codec's complete observable output before the native-tier
/// refactor begins. For each fixture the harness runs `NxCodec::decode` and
/// `NxCodec::inspect`, then serializes the full [`DecodeResult`] (the decoded
/// `CadIr` including the `nx` native-namespace arenas, the [`DecodeReport`], and
/// the [`SourceFidelity`] sidecar carrying provenance/exactness annotations) plus
/// the [`ContainerSummary`] into one deterministic pretty-JSON document, compared
/// byte-for-byte against a committed golden file under `tests/golden/`.
///
/// Serialization goes through `serde_json::to_value` (whose object maps are
/// `BTreeMap`, so keys sort) and then `to_string_pretty`. Every IR container that
/// reaches the wire is `BTreeMap`- or `Vec`-backed and codec output is sorted by
/// id, so the bytes are stable across runs; `golden_output_is_deterministic`
/// asserts that directly.
///
/// Regenerate after an intended output change with:
///   `UPDATE_GOLDEN=1 cargo test-fast golden`
/// then review the golden diff before committing. Regenerate with the workspace
/// feature set (`test-fast` / `--workspace`), NOT `-p cadmpeg-codec-nx`: the
/// fixtures zlib-compress their streams through `flate2`, and Cargo feature
/// unification selects the `zlib-rs` backend for the full-workspace build but
/// `miniz_oxide` for an isolated crate build. The two backends emit different
/// compressed bytes, so the container byte length, `sha256`, and byte-ledger
/// totals in these snapshots are only stable under the workspace build (the one
/// the commit hook and CI run). This is a build-config sensitivity of the
/// fixtures, not codec nondeterminism: `golden_output_is_deterministic` confirms
/// decode output is a pure function of the input bytes.
mod golden {
    use std::collections::BTreeSet;
    use std::io::Cursor;
    use std::path::{Path, PathBuf};

    use cadmpeg_ir::codec::{Codec, DecodeOptions};

    use super::*;

    /// Every arena name production writes via `set_arena` in `decode.rs`, extracted
    /// mechanically. This is the coverage denominator; `arena_coverage_is_a_subset`
    /// fails if production introduces an arena name this list does not know, which
    /// keeps the denominator honest as the code evolves.
    const KNOWN_ARENAS: &[&str] = &[
        "class_definitions",
        "configuration_attribute_uses",
        "configurations",
        "data_block_abr_reference_lanes",
        "data_block_column_index_tables",
        "data_block_control_class_references",
        "data_block_control_handle_pairs",
        "data_block_control_index_values",
        "data_block_control_references",
        "data_block_control_values",
        "data_block_counted_index_lanes",
        "data_block_index_rows",
        "data_block_linked_index_rows",
        "data_block_object_frames",
        "data_block_references",
        "data_block_target_index_rows",
        "data_blocks",
        "display_jt_base_node_data",
        "display_jt_compressed_element_sequences",
        "display_jt_compressed_elements",
        "display_jt_coordinate_array_headers",
        "display_jt_documents",
        "display_jt_geometric_transform_attributes",
        "display_jt_group_node_data",
        "display_jt_indices",
        "display_jt_initial_face_degree_symbols",
        "display_jt_instance_nodes",
        "display_jt_partition_nodes",
        "display_jt_polygon_meshes",
        "display_jt_range_lod_nodes",
        "display_jt_segments",
        "display_jt_shape_lod_bindings",
        "display_jt_shape_lod_elements",
        "display_jt_string_property_atoms",
        "display_jt_topology_packet_sequences",
        "display_jt_tri_strip_lod_headers",
        "display_jt_tri_strip_shape_nodes",
        "display_jt_vertex_colors",
        "display_jt_vertex_coordinates",
        "display_jt_vertex_flags",
        "display_jt_vertex_normals",
        "display_jt_vertex_records_headers",
        "display_jt_vertex_texture_coordinates",
        "expression_declarations",
        "expressions",
        "external_reference_empty_records",
        "external_reference_indexed_records",
        "external_reference_record_children",
        "external_reference_record_string_uses",
        "external_reference_records",
        "external_reference_tail_reference_pairs",
        "external_references",
        "feature_block_construction_payloads",
        "feature_block_construction_references",
        "feature_block_constructions",
        "feature_block_dimensions",
        "feature_block_payload_named_records",
        "feature_block_payload_names",
        "feature_block_payload_point_groups",
        "feature_block_payload_points",
        "feature_block_payload_scalars",
        "feature_body_reference_occurrences",
        "feature_body_references",
        "feature_body_segment_uses",
        "feature_boolean_operations",
        "feature_datum_csys_block_uses",
        "feature_datum_csys_constructions",
        "feature_datum_csys_descriptors",
        "feature_datum_csys_payload_fixed_pairs",
        "feature_datum_csys_payload_scalar_pairs",
        "feature_datum_csys_payload_scalars",
        "feature_datum_csys_payloads",
        "feature_datum_plane_block_uses",
        "feature_datum_plane_csys_identity_uses",
        "feature_datum_plane_descriptors",
        "feature_datum_plane_headers",
        "feature_datum_plane_payload_scalar_pairs",
        "feature_datum_plane_payloads",
        "feature_draft_construction_binary32_lanes",
        "feature_draft_construction_fixed_lanes",
        "feature_draft_construction_graph_payloads",
        "feature_draft_construction_graph_strings",
        "feature_draft_construction_identity_frames",
        "feature_draft_construction_index_lanes",
        "feature_draft_construction_payloads",
        "feature_draft_construction_references",
        "feature_draft_construction_terminal_lanes",
        "feature_extrude_32_constructions",
        "feature_extrude_construction_profiles",
        "feature_extrude_payload_32_branches",
        "feature_extrude_payload_footers",
        "feature_extrude_payload_headers",
        "feature_extrude_profile_references",
        "feature_input_block_identity_groups",
        "feature_input_blocks",
        "feature_input_column_row_uses",
        "feature_input_column_targets",
        "feature_operation_body_11_continuations",
        "feature_operation_body_members",
        "feature_operation_body_operands",
        "feature_operation_body_reference_lanes",
        "feature_operation_body_scalar_triples",
        "feature_operation_labels",
        "feature_operation_records",
        "feature_parameter_bindings",
        "feature_parameter_uses",
        "feature_pattern_construction_fixed_lanes",
        "feature_pattern_construction_payloads",
        "feature_pattern_construction_strings",
        "feature_pattern_references",
        "feature_pattern_transform_lanes",
        "feature_payload_strings",
        "feature_point_construction_headers",
        "feature_point_construction_scalar_lanes",
        "feature_projected_curve_construction_payloads",
        "feature_projected_curve_construction_strings",
        "feature_projected_curve_references",
        "feature_simple_hole_construction_groups",
        "feature_simple_hole_repeated_scalar_lane_block_references",
        "feature_simple_hole_repeated_scalar_lanes",
        "feature_simple_hole_templates",
        "feature_sketch_construction_inputs",
        "feature_sketch_construction_payloads",
        "feature_sketch_datum_csys_dependencies",
        "feature_sketch_fixed_points",
        "feature_sketch_named_point_block_uses",
        "feature_sketch_payload_coordinate_pairs",
        "feature_sketch_payload_fixed_pairs",
        "feature_sketch_payload_named_records",
        "feature_sketch_payload_names",
        "feature_sketch_payload_scalars",
        "feature_sketch_point_groups",
        "feature_sketch_point_uses",
        "feature_sketch_points",
        "feature_sketch_preceding_named_point_uses",
        "feature_sketch_records",
        "feature_sketch_references",
        "feature_surface_construction_branches",
        "feature_surface_construction_payloads",
        "feature_surface_construction_references",
        "feature_surface_construction_scalar_pairs",
        "feature_surface_construction_strings",
        "field_definitions",
        "material_texture_assets",
        "material_texture_catalog_entries",
        "object_records",
        "object_references",
        "offset_store_named_points",
        "om_record_areas",
        "parasolid_attribute_class_uses",
        "parasolid_attribute_definitions",
        "parasolid_blend_bound_records",
        "parasolid_blend_surface_records",
        "parasolid_chart_records",
        "parasolid_entity_51_numeric_uses",
        "parasolid_entity_51_records",
        "parasolid_entity_51_string_uses",
        "parasolid_entity_52_integer_records",
        "parasolid_entity_53_double_records",
        "parasolid_entity_54_string_records",
        "parasolid_intersection_records",
        "parasolid_offset_surface_records",
        "parasolid_support_uv_records",
        "parasolid_surface_curve_records",
        "parasolid_term_use_records",
        "parasolid_topology_attribute_class_uses",
        "parasolid_topology_attribute_list_references",
        "parasolid_trimmed_curve_records",
        "part_attributes",
        "persistent_handles",
        "rmfastload_object_id_tables",
        "rmfastload_object_ids",
        "segment_body_bindings",
        "segment_body_lineage_statuses",
        "segment_index_rows",
        "segment_om_links",
        "segment_stream_links",
        "store_headers",
        "string_values",
    ];

    /// A floor on distinct arenas the golden fixtures collectively populate.
    /// Frozen from the generated snapshots; if a refactor drops an arena from
    /// every fixture, `arena_coverage_meets_floor` fails. Raise it (never lower
    /// it) when new covering fixtures are added.
    const ARENA_COVERAGE_FLOOR: usize = 122;

    /// Build the covering fixture set: `(golden name, full `.prt` bytes)`. Each
    /// stream builder is wrapped exactly as its originating white-box test wraps
    /// it (`prt_with_partition` for a lone partition, `prt_with_streams` for a
    /// partition paired with an equal-schema deltas stream, `prt_with_named_payloads`
    /// for an OM record area), so the bytes exercise the real decode path.
    fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
        let mut f: Vec<(&'static str, Vec<u8>)> = Vec::new();

        // Self-contained `.prt` images.
        f.push(("single_part_prt", single_part_prt()));
        f.push(("topology_part_prt", topology_part_prt()));
        f.push(("prt_with_arrangements", prt_with_arrangements()));
        f.push((
            "prt_with_arrangement_attribute_none",
            prt_with_arrangement_attribute(None),
        ));
        f.push(("prt_with_indexed_om_section", prt_with_indexed_om_section()));
        f.push((
            "prt_with_size_framed_om_section",
            prt_with_size_framed_om_section(),
        ));
        f.push(("assembly_prt", assembly_prt()));
        f.push((
            "assembly_with_external_paths",
            assembly_with_external_paths(),
        ));
        f.push(("rmfastload_prt", rmfastload_prt()));
        f.push((
            "prt_with_two_bodies_and_rmfastload",
            prt_with_two_bodies_and_rmfastload(),
        ));
        f.push((
            "prt_with_two_active_bodies_and_rmfastload",
            prt_with_two_active_bodies_and_rmfastload(),
        ));
        f.push((
            "prt_with_missing_active_body_record",
            prt_with_missing_active_body_record(),
        ));
        f.push((
            "prt_with_weak_rmfastload_overlap",
            prt_with_weak_rmfastload_overlap(),
        ));

        // Parasolid neutral-binary attribute/entity records in a partition stream.
        f.push((
            "parasolid_entity_records",
            prt_with_partition(&parasolid_entity_records_stream()),
        ));

        // Embedded DisplayJT stream: outer index, one JT document, one segment.
        f.push((
            "display_jt_basic",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                ("/Root/UG_PART/DisplayJT", display_jt_basic_stream()),
            ]),
        ));
        f.push((
            "display_jt_scene_graph",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                ("/Root/UG_PART/DisplayJT", display_jt_scene_graph_stream()),
            ]),
        ));
        f.push((
            "display_jt_shape_lod",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                ("/Root/UG_PART/DisplayJT", display_jt_shape_lod_stream()),
            ]),
        ));
        f.push((
            "display_jt_string_property",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                (
                    "/Root/UG_PART/DisplayJT",
                    display_jt_string_property_stream(),
                ),
            ]),
        ));

        // Offset-store control blocks: the plain form resolves class-registry
        // ordinals; the handle form carries two adjacent persistent handles.
        f.push((
            "data_block_control_class_references",
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", offset_only_indexed_om_section())]),
        ));
        f.push((
            "offset_store_named_point",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                offset_only_indexed_om_section_with_named_point(),
            )]),
        ));
        f.push((
            "data_block_control_index_values",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                offset_only_indexed_om_section_with_index_values(),
            )]),
        ));
        // EXTREFSTREAM index, string table, and handle-set records.
        f.push((
            "external_reference_stream",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                ("/Root/ExternalReferences", external_reference_stream()),
            ]),
        ));

        f.push(("data_block_control_handles", {
            let mut control = Vec::new();
            control.extend_from_slice(&[0xe0, 0, 0, 0, 1]);
            control.extend_from_slice(&[0xe0, 0, 0, 0, 2]);
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                offset_only_indexed_om_section_with_control(&control),
            )])
        }));

        // OM record areas / feature history, wrapped as a named UG_PART payload.
        f.push((
            "om_record_area",
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_om_record_area_payload())]),
        ));
        f.push((
            "om_record_area_input_store",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                segment_om_record_area_with_input_store_payload(),
            )]),
        ));
        f.push((
            "multi_section_feature_history",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                multi_section_feature_history_payload(),
            )]),
        ));
        f.push(("composed_feature_history", composed_feature_history_prt()));
        f.push((
            "segment_index_rows",
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_index_payload())]),
        ));
        f.push((
            "segment_stream_links",
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", segment_stream_payload())]),
        ));
        f.push((
            "segment_body_bindings",
            prt_with_named_payloads(&[(
                "/Root/UG_PART/UG_PART",
                segment_body_binding_payload("partition"),
            )]),
        ));
        f.push((
            "material_texture_assets",
            prt_with_named_payloads(&[
                ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
                (
                    "/Root/materialsTif/AISI Steel 4340",
                    vec![b'I', b'I', 42, 0, 8, 0, 0, 0, 0, 0],
                ),
                (
                    "/Root/materialsTif/Truncated",
                    vec![b'I', b'I', 42, 0, 40, 0, 0, 0, 0, 0],
                ),
            ]),
        ));
        f.push(("material_texture_catalog", prt_with_named_payloads(&[
            ("/Root/UG_PART/UG_PART", zlib_compress(&partition_stream())),
            ("/Root/materialsTif/unmap$1", vec![b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 0]),
            ("/Root/qafmetadata", br#"<?xml version="1.0" encoding="UTF-8"?>
<folderContents>
<folderProperties location="images/preview" unmappedLocation="images/preview"><createTime>2026-07-15T08:00:00</createTime><modifyTime>2026-07-15T08:00:01</modifyTime></folderProperties>
<folderProperties location="materialsTif/unmap$1" unmappedLocation="materialsTif/Carbon Fiber Harness Satin Coated"><createTime>2026-07-15T08:01:00</createTime><modifyTime>2026-07-15T08:02:00</modifyTime></folderProperties>
</folderContents>"#.to_vec()),
        ])));
        f.push(("om_repeated_operations", {
            let section = size_framed_om_section_with_repeated_operations(12);
            let mut payload = Vec::new();
            for word in [24_u32, 9, 11, 1, 1, 24] {
                payload.extend_from_slice(&word.to_le_bytes());
            }
            payload.extend_from_slice(&section);
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)])
        }));

        // Lone partition streams, each wrapped with `prt_with_partition`.
        let partitions: Vec<(&'static str, Vec<u8>)> = vec![
            ("topology_partition_stream", topology_partition_stream()),
            (
                "topology_with_missing_tolerances",
                topology_with_missing_tolerances(),
            ),
            ("partition_stream", partition_stream()),
            (
                "offset_surface_topology_partition_stream",
                offset_surface_topology_partition_stream(),
            ),
            (
                "offset_surface_with_fully_extended_common_header",
                offset_surface_with_fully_extended_common_header(),
            ),
            (
                "surface_curve_topology_partition_stream",
                surface_curve_topology_partition_stream(),
            ),
            (
                "pcurve_topology_partition_stream",
                pcurve_topology_partition_stream(),
            ),
            (
                "shared_region_shells_partition_stream",
                shared_region_shells_partition_stream(),
            ),
            (
                "blend_surface_topology_partition_stream",
                blend_surface_topology_partition_stream(),
            ),
            (
                "blend_surface_with_extended_support_reference",
                blend_surface_with_extended_support_reference(),
            ),
            (
                "blend_surface_with_intersection_spine",
                blend_surface_with_intersection_spine(),
            ),
            (
                "blend_surface_with_forward_blend_support",
                blend_surface_with_forward_blend_support(),
            ),
            (
                "intersection_curve_topology_partition_stream",
                intersection_curve_topology_partition_stream(),
            ),
            (
                "charted_intersection_curve_topology_partition_stream",
                charted_intersection_curve_topology_partition_stream(),
            ),
            (
                "charted_intersection_with_edge_endpoint_witnesses_stream",
                charted_intersection_with_edge_endpoint_witnesses_stream(),
            ),
            (
                "charted_intersection_without_uv_stream",
                charted_intersection_without_uv_stream(),
            ),
            (
                "charted_intersection_with_approximated_term_stream",
                charted_intersection_with_approximated_term_stream(),
            ),
            (
                "ext11_charted_intersection_curve_stream",
                ext11_charted_intersection_curve_stream(),
            ),
            (
                "two_support_ext11_charted_intersection_curve_stream",
                two_support_ext11_charted_intersection_curve_stream(false),
            ),
            (
                "two_support_ext11_charted_intersection_curve_stream_ambiguous",
                two_support_ext11_charted_intersection_curve_stream(true),
            ),
            (
                "partial_ext11_charted_intersection_curve_stream",
                partial_ext11_charted_intersection_curve_stream(),
            ),
            (
                "two_support_charted_intersection_curve_stream",
                two_support_charted_intersection_curve_stream(),
            ),
            (
                "blend_bound_charted_intersection_curve_stream",
                blend_bound_charted_intersection_curve_stream(),
            ),
            (
                "inline_descriptor_intersection_curve_stream",
                inline_descriptor_intersection_curve_stream(),
            ),
            (
                "circle_topology_partition_stream",
                circle_topology_partition_stream(),
            ),
            (
                "ellipse_topology_partition_stream",
                ellipse_topology_partition_stream(),
            ),
            (
                "cylinder_topology_partition_stream",
                cylinder_topology_partition_stream(),
            ),
            (
                "cone_topology_partition_stream",
                cone_topology_partition_stream(),
            ),
            (
                "sphere_topology_partition_stream",
                sphere_topology_partition_stream(),
            ),
            (
                "torus_topology_partition_stream",
                torus_topology_partition_stream(),
            ),
            ("bspline_partition_stream", bspline_partition_stream()),
            (
                "extended_bspline_surface_stream",
                extended_bspline_surface_stream(),
            ),
            (
                "bspline_surface_replacement_partition_stream",
                bspline_surface_replacement_partition_stream(),
            ),
            (
                "bspline_curve_replacement_partition_stream",
                bspline_curve_replacement_partition_stream(),
            ),
            (
                "trimmed_topology_partition_stream",
                trimmed_topology_partition_stream(),
            ),
            (
                "mismatched_trimmed_topology_partition_stream",
                mismatched_trimmed_topology_partition_stream(),
            ),
            (
                "partnered_trimmed_topology_partition_stream",
                partnered_trimmed_topology_partition_stream(),
            ),
            (
                "forward_trimmed_curve_chain_stream",
                forward_trimmed_curve_chain_stream(),
            ),
            (
                "topology_with_extended_edge_curve_reference",
                topology_with_extended_edge_curve_reference(),
            ),
            (
                "topology_with_extended_face_attribute_reference",
                topology_with_extended_face_attribute_reference(),
            ),
            (
                "topology_with_extended_edge_attribute_reference",
                topology_with_extended_edge_attribute_reference(),
            ),
            (
                "topology_with_extended_internal_topology_references",
                topology_with_extended_internal_topology_references(),
            ),
            (
                "topology_with_fully_extended_geometry_headers",
                topology_with_fully_extended_geometry_headers(),
            ),
            (
                "topology_with_escaped_geometry_envelopes",
                topology_with_escaped_geometry_envelopes(),
            ),
            (
                "deltas_intersection_curve_stream",
                deltas_intersection_curve_stream(),
            ),
            ("status_framed_deltas_stream", status_framed_deltas_stream()),
            (
                "variable_status_framed_deltas_stream",
                variable_status_framed_deltas_stream(),
            ),
            (
                "status_framed_deltas_point_stream",
                status_framed_deltas_point_stream(),
            ),
            (
                "deltas_point_partition_stream",
                deltas_point_partition_stream(),
            ),
            ("many_face_partition_stream", many_face_partition_stream(1)),
            (
                "large_xmt_headers_topology",
                large_xmt_headers(&topology_partition_stream()),
            ),
        ];
        for (name, stream) in partitions {
            f.push((name, prt_with_partition(&stream)));
        }

        // Deltas streams paired with an equal-schema partition via `prt_with_streams`.
        let deltas_pairs: Vec<(&'static str, Vec<u8>, Vec<u8>)> = vec![
            (
                "deltas_edge",
                topology_partition_stream(),
                deltas_edge_partition_stream(),
            ),
            (
                "deltas_face_vertex",
                topology_partition_stream(),
                deltas_face_vertex_partition_stream(),
            ),
            (
                "deltas_loop",
                topology_partition_stream(),
                deltas_loop_partition_stream(),
            ),
            (
                "deltas_shell",
                topology_partition_stream(),
                deltas_shell_partition_stream(),
            ),
            (
                "deltas_fin",
                topology_partition_stream(),
                deltas_fin_partition_stream(),
            ),
            (
                "deltas_line",
                topology_partition_stream(),
                deltas_line_partition_stream(),
            ),
            (
                "deltas_plane",
                topology_partition_stream(),
                deltas_plane_partition_stream(),
            ),
            (
                "deltas_offset_surface",
                offset_surface_topology_partition_stream(),
                deltas_offset_surface_partition_stream(),
            ),
            (
                "deltas_blend_surface",
                blend_surface_topology_partition_stream(),
                deltas_blend_surface_partition_stream(),
            ),
            (
                "deltas_trimmed_curve",
                trimmed_topology_partition_stream(),
                deltas_trimmed_curve_partition_stream(),
            ),
            (
                "deltas_surface_curve",
                surface_curve_topology_partition_stream(),
                deltas_surface_curve_partition_stream(),
            ),
            (
                "deltas_circle",
                circle_topology_partition_stream(),
                deltas_circle_partition_stream(),
            ),
            (
                "deltas_ellipse",
                ellipse_topology_partition_stream(),
                deltas_ellipse_partition_stream(),
            ),
            (
                "deltas_cylinder",
                cylinder_topology_partition_stream(),
                deltas_cylinder_partition_stream(),
            ),
            (
                "deltas_cone",
                cone_topology_partition_stream(),
                deltas_cone_partition_stream(),
            ),
            (
                "deltas_sphere",
                sphere_topology_partition_stream(),
                deltas_sphere_partition_stream(),
            ),
            (
                "deltas_torus",
                torus_topology_partition_stream(),
                deltas_torus_partition_stream(),
            ),
            (
                "deltas_bspline_surface",
                bspline_surface_replacement_partition_stream(),
                deltas_bspline_surface_wrapper_stream(),
            ),
            (
                "deltas_bspline_curve",
                bspline_curve_replacement_partition_stream(),
                deltas_bspline_curve_wrapper_stream(),
            ),
        ];
        for (name, partition, delta) in deltas_pairs {
            f.push((name, prt_with_streams(&[&partition, &delta])));
        }

        f
    }

    /// Serialize the complete decode + inspect output for one fixture as stable
    /// pretty JSON. Decode/inspect errors are frozen too (a `.prt` that fails to
    /// decode is a real, contract-relevant behavior), so this never panics on
    /// codec output.
    fn snapshot(bytes: &[u8]) -> String {
        let decode =
            match NxCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default()) {
                Ok(result) => serde_json::json!({
                    "ir": serde_json::to_value(&result.ir).expect("serialize ir"),
                    "report": serde_json::to_value(&result.report).expect("serialize report"),
                    "source_fidelity": serde_json::to_value(&result.source_fidelity)
                        .expect("serialize source_fidelity"),
                }),
                Err(err) => serde_json::json!({ "decode_error": err.to_string() }),
            };
        let inspect = match NxCodec.inspect(&mut Cursor::new(bytes.to_vec())) {
            Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect"),
            Err(err) => serde_json::json!({ "inspect_error": err.to_string() }),
        };
        let combined = serde_json::json!({ "decode": decode, "inspect": inspect });
        let mut text = serde_json::to_string_pretty(&combined).expect("serialize snapshot");
        text.push('\n');
        text
    }

    fn golden_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
    }

    fn golden_path(name: &str) -> PathBuf {
        golden_dir().join(format!("{name}.json"))
    }

    /// First line that differs between two documents, 1-based, with both sides
    /// truncated for a readable failure. `None` when the shorter side is a prefix
    /// of the longer (length-only difference).
    fn first_line_diff(expected: &str, actual: &str) -> (usize, String, String) {
        let mut exp = expected.lines();
        let mut act = actual.lines();
        let mut line = 0usize;
        loop {
            line += 1;
            match (exp.next(), act.next()) {
                (Some(e), Some(a)) if e == a => {}
                (e, a) => {
                    let trunc = |s: Option<&str>| match s {
                        Some(s) if s.len() > 200 => format!("{}…", &s[..200]),
                        Some(s) => s.to_string(),
                        None => "<end of file>".to_string(),
                    };
                    return (line, trunc(e), trunc(a));
                }
            }
        }
    }

    fn update_requested() -> bool {
        std::env::var_os("UPDATE_GOLDEN").is_some()
    }

    #[test]
    fn golden_snapshots_are_byte_identical() {
        let update = update_requested();
        if update {
            std::fs::create_dir_all(golden_dir()).expect("create golden dir");
        }
        let mut failures: Vec<String> = Vec::new();
        for (name, bytes) in fixtures() {
            let actual = snapshot(&bytes);
            let path = golden_path(name);
            if update {
                std::fs::write(&path, actual.as_bytes())
                    .unwrap_or_else(|e| panic!("write golden {name}: {e}"));
                continue;
            }
            let expected = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) => {
                    failures.push(format!(
                        "fixture `{name}`: cannot read golden {} ({e}); run `UPDATE_GOLDEN=1 cargo test-fast golden`",
                        path.display()
                    ));
                    continue;
                }
            };
            if expected != actual {
                let (line, exp_line, act_line) = first_line_diff(&expected, &actual);
                failures.push(format!(
                    "fixture `{name}`: output diverged from golden at line {line}\n    golden: {exp_line}\n    actual: {act_line}"
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "{} golden snapshot(s) drifted; if the change is intended run `UPDATE_GOLDEN=1 cargo test-fast golden` and review the diff:\n\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }

    /// Guards against nondeterministic codec output (`HashMap` iteration order,
    /// timestamps): decoding the same bytes twice must produce identical JSON.
    #[test]
    fn golden_output_is_deterministic() {
        for (name, bytes) in fixtures() {
            let first = snapshot(&bytes);
            let second = snapshot(&bytes);
            if first != second {
                let (line, a, b) = first_line_diff(&first, &second);
                panic!("fixture `{name}`: nondeterministic output at line {line}\n    run 1: {a}\n    run 2: {b}");
            }
        }
    }

    /// Union of `nx`-namespace arenas the fixture set populates.
    fn covered_arenas() -> BTreeSet<String> {
        let mut covered = BTreeSet::new();
        for (_, bytes) in fixtures() {
            let Ok(result) = NxCodec.decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            else {
                continue;
            };
            if let Some(namespace) = result.ir.native.namespace("nx") {
                for (arena, records) in &namespace.arenas {
                    if !records.is_empty() {
                        covered.insert(arena.clone());
                    }
                }
            }
        }
        covered
    }

    /// Every arena a fixture populates must be a name production actually writes.
    /// A failure here means `KNOWN_ARENAS` (the coverage denominator) is stale.
    #[test]
    fn arena_coverage_is_a_subset() {
        let known: BTreeSet<&str> = KNOWN_ARENAS.iter().copied().collect();
        let unknown: Vec<String> = covered_arenas()
            .into_iter()
            .filter(|a| a != "unknowns" && !known.contains(a.as_str()))
            .collect();
        assert!(
            unknown.is_empty(),
            "fixtures populated arenas absent from KNOWN_ARENAS (update the denominator): {unknown:?}"
        );
    }

    /// Freezes the collective arena coverage floor so a refactor cannot silently
    /// stop populating an arena across the whole fixture set. Prints the fraction
    /// under `--nocapture`.
    #[test]
    fn arena_coverage_meets_floor() {
        let covered = covered_arenas();
        let known: BTreeSet<&str> = KNOWN_ARENAS.iter().copied().collect();
        let hit = covered
            .iter()
            .filter(|a| known.contains(a.as_str()))
            .count();
        let uncovered: Vec<&str> = KNOWN_ARENAS
            .iter()
            .copied()
            .filter(|a| !covered.contains(*a))
            .collect();
        println!(
            "golden arena coverage: {hit}/{} known arenas ({:.1}%)\nuncovered: {uncovered:?}",
            KNOWN_ARENAS.len(),
            100.0 * hit as f64 / KNOWN_ARENAS.len() as f64,
        );
        assert!(
            hit >= ARENA_COVERAGE_FLOOR,
            "arena coverage regressed: {hit} < floor {ARENA_COVERAGE_FLOOR}"
        );
    }

    /// The catalogue is the single source of truth for arena names: every arena
    /// appears exactly once across `CATALOGUE`, there is one row per model field
    /// (179), and the catalogue's arena set is exactly `KNOWN_ARENAS`. The exact
    /// equality is the relationship the fixtures confirm — every arena a fixture
    /// can populate is a catalogue arena, and every catalogue arena is a name
    /// `KNOWN_ARENAS` tracks. A single production site (`native::attach`) emits
    /// arenas, all of them catalogue-driven, so no non-catalogued arena exists.
    #[test]
    fn catalogue_arenas_match_known_arenas() {
        use crate::native::catalogue::CATALOGUE;

        assert_eq!(CATALOGUE.len(), 179, "one catalogue row per model field");

        let mut catalogue_arenas = BTreeSet::new();
        for row in CATALOGUE {
            assert!(
                catalogue_arenas.insert(row.arena),
                "arena {:?} appears in more than one catalogue row",
                row.arena
            );
        }
        assert_eq!(
            catalogue_arenas.len(),
            CATALOGUE.len(),
            "every catalogue row owns a distinct arena"
        );

        let known: BTreeSet<&str> = KNOWN_ARENAS.iter().copied().collect();
        let catalogue_not_known: Vec<&str> = catalogue_arenas.difference(&known).copied().collect();
        let known_not_catalogue: Vec<&str> = known.difference(&catalogue_arenas).copied().collect();
        assert!(
            catalogue_not_known.is_empty(),
            "catalogue arenas absent from KNOWN_ARENAS: {catalogue_not_known:?}"
        );
        assert!(
            known_not_catalogue.is_empty(),
            "KNOWN_ARENAS entries absent from CATALOGUE: {known_not_catalogue:?}"
        );
    }
}
