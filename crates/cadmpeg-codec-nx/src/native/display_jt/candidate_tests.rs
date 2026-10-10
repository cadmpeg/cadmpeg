// SPDX-License-Identifier: Apache-2.0
//! Storage release and visited-prefix admission for rejected JT candidates.

use super::*;
use cadmpeg_core::decode::ResourceDimension;

fn partition_body(name: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    let units: Vec<_> = name.encode_utf16().collect();
    body.extend_from_slice(&u32::try_from(units.len()).unwrap().to_le_bytes());
    for unit in units {
        body.extend_from_slice(&unit.to_le_bytes());
    }
    for _ in 0..13 {
        body.extend_from_slice(&0_f32.to_le_bytes());
    }
    for _ in 0..6 {
        body.extend_from_slice(&0_i32.to_le_bytes());
    }
    body
}

pub(super) fn assert_released(decode: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>) {
    const ALLOWANCE: u64 = 1 << 20;
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = ALLOWANCE;
            policy.limits.max_materialized_bytes = ALLOWANCE;
        },
        |ctx| {
            decode(ctx).unwrap();
            let scratch = ctx
                .reserve_scoped(ALLOWANCE, "verify candidate scratch release")
                .unwrap();
            drop(scratch);
            let storage = ctx
                .vector_storage::<u8>(
                    usize::try_from(ALLOWANCE).unwrap(),
                    "verify candidate retained release",
                )
                .unwrap();
            assert_eq!(storage.capacity(), usize::try_from(ALLOWANCE).unwrap());
        },
    );
}

#[test]
fn partition_candidate_releases_children_and_name_after_tail_rejection() {
    let mut body = partition_body("x");
    crate::test_support::with_decode_context(|ctx| {
        let node = parse_jt9_partition_node_body(ctx, &body).unwrap().unwrap();
        assert_eq!(node.child_object_ids, [7]);
        assert_eq!(node.file_name, "x");
    });
    body.push(1);
    assert_released(|ctx| {
        assert!(parse_jt9_partition_node_body(ctx, &body)?.is_none());
        Ok(())
    });
}

#[test]
fn partition_name_control_rejection_charges_only_the_first_character() {
    let body = partition_body(&format!("\n{}", "x".repeat(128)));
    let operation = "validate DisplayJT partition name";
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        operation,
        |ctx| parse_jt9_partition_node_body(ctx, &body).map(|_| ()),
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("work refusal");
    };
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.additional, 1);
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = limit.used + 1,
        |ctx| assert!(parse_jt9_partition_node_body(ctx, &body).unwrap().is_none()),
    );
}

fn container(data: Vec<u8>) -> crate::container::Container<'static> {
    use crate::container::{Container, DirEntry, DirEntryBody, Region};
    let len = cadmpeg_core::decode::u64_from_index(data.len());
    Container {
        data: data.into(),
        physical_size: len,
        layout: crate::container::test_modern_layout(6),
        entries: vec![DirEntry {
            name: "/Root/UG_PART/DisplayJT".into(),
            region: Region::Footer,
            body: DirEntryBody::File { offset: 0, len },
        }],
        fastload_table: None,
        segment_index: None,
        segment_wrappers: Vec::new(),
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    }
}

fn segment(data: &[u8], kind: u32, compression: Option<DisplayJtCompression>) -> DisplayJtSegment {
    DisplayJtSegment {
        id: "segment".into(),
        document: "document".into(),
        toc_entry: "toc-entry".into(),
        segment_id: [0; 16],
        segment_type: kind,
        segment_byte_len: u32::try_from(data.len()).unwrap(),
        payload_sha256: Sha256Digest::digest(&[]),
        compression,
        source_offset: 0,
    }
}

fn framed_element(kind: [u8; 16], base: u8, body: &[u8]) -> Vec<u8> {
    let mut bytes = u32::try_from(21 + body.len())
        .unwrap()
        .to_le_bytes()
        .to_vec();
    bytes.extend_from_slice(&kind);
    bytes.push(base);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(body);
    bytes
}

fn end_marker(bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&[0xff; 16]);
}

fn compressed_fixture(
    expanded: &[u8],
    kind: u32,
) -> (crate::container::Container<'static>, DisplayJtSegment) {
    let compressed = crate::test_support::test_bytes::zlib_compress_at_level(expanded, 1);
    let mut bytes = vec![0; 33];
    bytes.extend_from_slice(&compressed);
    let compression = DisplayJtCompression {
        envelope: JtCompressionEnvelope::try_new(
            2,
            u32::try_from(compressed.len() + 1).unwrap(),
            2,
            u32::try_from(compressed.len()).unwrap(),
        )
        .unwrap(),
        inflated_sha256: Sha256Digest::digest(expanded),
    };
    let segment = segment(&bytes, kind, Some(compression));
    (container(bytes), segment)
}

const TRI_STRIP_TYPE: [u8; 16] = [
    0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0, 0x80, 0xc7, 0xbb, 0x59, 0x97,
];
const STRING_TYPE: [u8; 16] = [
    0x6e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0, 0x80, 0xc7, 0xbb, 0x59, 0x97,
];

fn tri_strip_body(representation: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u64.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(representation);
    body
}

fn shape_fixture(
    body: &[u8],
) -> (
    crate::container::Container<'static>,
    Vec<DisplayJtShapeLodElement>,
) {
    let mut bytes = vec![0; 25];
    bytes.extend_from_slice(body);
    let element = DisplayJtShapeLodElement {
        id: "shape".into(),
        segment: "segment".into(),
        ordinal: 0,
        object_type_id: TRI_STRIP_TYPE,
        object_id: 7,
        body_byte_len: u32::try_from(body.len()).unwrap(),
        body_sha256: Sha256Digest::digest(body),
        source_offset: 0,
    };
    let mut invalid = element.clone();
    invalid.source_offset = u64::MAX;
    (container(bytes), vec![element, invalid])
}

/// One zero value in fixed-width bitlength mode: mode, two six-bit zero widths.
fn zero_packet() -> Vec<u8> {
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.push(1);
    bytes.extend_from_slice(&13_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes
}

#[test]
fn range_vector_candidate_releases_valid_prefix_after_nan() {
    let mut bytes = 2_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&1_f32.to_le_bytes());
    bytes.extend_from_slice(&f32::NAN.to_le_bytes());
    assert_released(|ctx| {
        assert!(parse_jt_f32_vector(ctx, &mut View::over_retained(&bytes))?.is_none());
        Ok(())
    });
}

#[test]
fn range_lod_candidate_releases_children_and_lanes_after_center_rejection() {
    let mut body = partition_body("x")[..20].to_vec();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&0_f32.to_le_bytes());
    body.extend_from_slice(&0_i32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&1_f32.to_le_bytes());
    body.extend_from_slice(&[0; 12]);
    crate::test_support::with_decode_context(|ctx| {
        assert!(parse_jt9_range_lod_node_body(ctx, &body).unwrap().is_some());
    });
    body.push(1);
    assert_released(|ctx| {
        assert!(parse_jt9_range_lod_node_body(ctx, &body)?.is_none());
        Ok(())
    });
}

#[test]
fn index_candidate_releases_valid_row_after_nonascending_offset() {
    let mut bytes = 9_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    for _ in 0..2 {
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&40_u32.to_le_bytes());
    }
    bytes.extend_from_slice(b"Version ");
    let container = container(bytes);
    assert_released(|ctx| {
        assert!(display_jt_indices(ctx, &container)?.is_empty());
        Ok(())
    });
}

#[test]
fn document_candidate_releases_valid_document_before_invalid_second_header() {
    let mut container = super::document_admission_tests::one_document();
    let mut indices = crate::test_support::with_decode_context(|ctx| {
        display_jt_indices(ctx, &container).unwrap()
    });
    let first = indices[0].rows.first().clone();
    let mut second = first.clone();
    second.id = "nx:display-jt:index#0-row-1".into();
    second.ordinal = 1;
    second.header_offset = 189;
    indices[0] = DisplayJtIndex::new(indices[0].id.clone(), 9, vec![first, second], 0).unwrap();
    container.data.to_mut().extend_from_slice(b"Version ");
    container.physical_size += 8;
    container.entries[0].body = crate::container::DirEntryBody::File {
        offset: 0,
        len: container.physical_size,
    };
    assert_released(|ctx| {
        assert!(display_jt_documents(ctx, &container, &indices)?.is_empty());
        Ok(())
    });
}

#[test]
fn segment_candidate_releases_valid_segment_before_uuid_mismatch() {
    let mut container = super::document_admission_tests::one_document();
    let data = container.data.to_mut();
    data[165..181].copy_from_slice(&[2; 16]);
    data[181..185].copy_from_slice(&1_u32.to_le_bytes());
    data[185..189].copy_from_slice(&24_u32.to_le_bytes());
    let mut documents = crate::test_support::with_decode_context(|ctx| {
        let indices = display_jt_indices(ctx, &container).unwrap();
        let documents = display_jt_documents(ctx, &container, &indices).unwrap();
        assert_eq!(
            display_jt_segments(ctx, &container, &documents)
                .unwrap()
                .len(),
            1
        );
        documents
    });
    let mut second = documents[0].toc_entries[0].clone();
    second.segment_id = [3; 16];
    documents[0].toc_entries.push(second);
    assert_released(|ctx| {
        assert!(display_jt_segments(ctx, &container, &documents)?.is_empty());
        Ok(())
    });
}

#[test]
fn shape_element_candidate_releases_valid_element_before_wrong_base_type() {
    let mut bytes = vec![0; 24];
    bytes.extend_from_slice(&framed_element([0; 16], 4, &[1]));
    bytes.extend_from_slice(&framed_element([0; 16], 5, &[1]));
    end_marker(&mut bytes);
    bytes.extend_from_slice(&[1, 0, 0, 0, 0, 0]);
    let segment = segment(&bytes, 7, None);
    let container = container(bytes);
    assert_released(|ctx| {
        assert!(
            display_jt_shape_lod_elements(ctx, &container, std::slice::from_ref(&segment))?
                .is_empty()
        );
        Ok(())
    });
}

#[test]
fn tri_strip_header_candidate_releases_valid_header_before_invalid_element() {
    let (container, elements) = shape_fixture(&tri_strip_body(&[]));
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            display_jt_tri_strip_lod_headers(ctx, &container, &elements[..1])
                .unwrap()
                .len(),
            1
        );
    });
    assert_released(|ctx| {
        assert!(display_jt_tri_strip_lod_headers(ctx, &container, &elements)?.is_empty());
        Ok(())
    });
}

#[test]
fn face_degree_candidate_releases_valid_record_before_invalid_element() {
    let (container, elements) = shape_fixture(&tri_strip_body(&zero_packet()));
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            display_jt_initial_face_degree_symbols(ctx, &container, &elements[..1]).unwrap()[0]
                .degrees,
            [0]
        );
    });
    assert_released(|ctx| {
        assert!(display_jt_initial_face_degree_symbols(ctx, &container, &elements)?.is_empty());
        Ok(())
    });
}

#[test]
fn topology_candidate_releases_valid_packets_before_invalid_element() {
    let mut representation = vec![0; 24 * 4];
    representation.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
    representation.extend_from_slice(&[0; 16]);
    let (container, elements) = shape_fixture(&tri_strip_body(&representation));
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            display_jt_topology_packet_sequences(ctx, &container, &elements[..1])
                .unwrap()
                .sequences
                .len(),
            1
        );
    });
    assert_released(|ctx| {
        let result = display_jt_topology_packet_sequences(ctx, &container, &elements)?;
        assert!(result.sequences.is_empty());
        assert!(result.vertex_headers.is_empty());
        assert!(result.coordinate_headers.is_empty());
        Ok(())
    });
}

pub(super) fn coordinate_header() -> DisplayJtVertexCoordinateArrayHeader {
    DisplayJtVertexCoordinateArrayHeader {
        id: "coordinate-header".into(),
        element: "shape".into(),
        unique_vertex_count: 1,
        component_count: 3,
        component_ranges: [QuantizedRange::ZERO; 3],
        component_quantization_bits: [0; 3],
        compressed_components_byte_len: 0,
        compressed_components_sha256: Sha256Digest::digest(&[]),
        source_offset: 0,
    }
}

pub(super) fn vertex_header(bindings: u64) -> DisplayJtCompressedVertexRecordsHeader {
    DisplayJtCompressedVertexRecordsHeader {
        id: "vertex-header".into(),
        element: "shape".into(),
        vertex_bindings: bindings,
        vertex_quantization_bits: 0,
        normal_quantization_factor: 1,
        texture_quantization_bits: 1,
        color_quantization_bits: 1,
        topological_vertex_count: 1,
        vertex_attribute_count: 1,
        compressed_arrays_byte_len: 0,
        compressed_arrays_sha256: Sha256Digest::digest(&[]),
        source_offset: 0,
    }
}

pub(super) fn coordinates() -> DisplayJtVertexCoordinates {
    DisplayJtVertexCoordinates {
        id: "coordinates".into(),
        header: "coordinate-header".into(),
        points_m: vec![[FiniteBinary32::ZERO; 3]],
        coordinate_hash: 0,
        byte_len: 0,
        source_offset: 0,
    }
}

#[test]
fn coordinate_candidate_releases_valid_array_before_invalid_component() {
    let mut bytes = vec![0; 32];
    for _ in 0..6 {
        bytes.extend_from_slice(&zero_packet());
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    let container = container(bytes);
    let mut first = coordinate_header();
    first.compressed_components_byte_len = 6 * 13 + 4;
    let mut second = first.clone();
    second.source_offset = u64::MAX;
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            display_jt_vertex_coordinates(ctx, &container, std::slice::from_ref(&first))
                .unwrap()
                .len(),
            1
        );
    });
    assert_released(|ctx| {
        assert!(
            display_jt_vertex_coordinates(ctx, &container, &[first.clone(), second.clone()])?
                .is_empty()
        );
        Ok(())
    });
}

#[test]
fn normal_candidate_releases_valid_array_before_missing_owner() {
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[3, 1]);
    for _ in 0..4 {
        bytes.extend_from_slice(&zero_packet());
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    let container = container(bytes);
    let first = vertex_header(0x8);
    let mut second = first.clone();
    second.element = "missing".into();
    let headers = [coordinate_header()];
    let coordinates = [coordinates()];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            display_jt_vertex_normals(
                ctx,
                &container,
                std::slice::from_ref(&first),
                &headers,
                &coordinates
            )
            .unwrap()
            .len(),
            1
        );
    });
    assert_released(|ctx| {
        assert!(display_jt_vertex_normals(
            ctx,
            &container,
            &[first.clone(), second.clone()],
            &headers,
            &coordinates
        )?
        .is_empty());
        Ok(())
    });
}

#[test]
fn color_candidate_releases_valid_array_before_missing_normal() {
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[4, 1, 0]);
    for _ in 0..4 {
        bytes.extend_from_slice(&[0; 8]);
        bytes.push(1);
    }
    for _ in 0..4 {
        bytes.extend_from_slice(&zero_packet());
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    let container = container(bytes);
    let first = vertex_header(0x30);
    let mut second = first.clone();
    second.vertex_bindings |= 0x8;
    let headers = [coordinate_header()];
    let coordinates = [coordinates()];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            display_jt_vertex_colors(
                ctx,
                &container,
                std::slice::from_ref(&first),
                &headers,
                &coordinates,
                &[]
            )
            .unwrap()
            .len(),
            1
        );
    });
    assert_released(|ctx| {
        assert!(display_jt_vertex_colors(
            ctx,
            &container,
            &[first.clone(), second.clone()],
            &headers,
            &coordinates,
            &[]
        )?
        .is_empty());
        Ok(())
    });
}

#[test]
fn texture_candidate_releases_valid_array_before_zero_attribute_count() {
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[1, 1]);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&zero_packet());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    let container = container(bytes);
    let first = vertex_header(0x100);
    let mut second = first.clone();
    second.vertex_attribute_count = 0;
    let headers = [coordinate_header()];
    let coordinates = [coordinates()];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            display_jt_vertex_texture_coordinates(
                ctx,
                &container,
                std::slice::from_ref(&first),
                &headers,
                &coordinates,
                &[],
                &[]
            )
            .unwrap()
            .len(),
            1
        );
    });
    assert_released(|ctx| {
        assert!(display_jt_vertex_texture_coordinates(
            ctx,
            &container,
            &[first.clone(), second.clone()],
            &headers,
            &coordinates,
            &[],
            &[]
        )?
        .is_empty());
        Ok(())
    });
}

#[test]
fn flag_candidate_releases_valid_array_before_missing_texture() {
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&zero_packet());
    let container = container(bytes);
    let first = vertex_header(0x40);
    let mut second = first.clone();
    second.vertex_bindings |= 0x100;
    let headers = [coordinate_header()];
    let coordinates = [coordinates()];
    let decode = |ctx: &DecodeContext<'_>, vertex_headers| {
        display_jt_vertex_flags(
            ctx,
            DisplayJtVertexFlagInputs {
                container: &container,
                vertex_headers,
                coordinate_headers: &headers,
                coordinates: &coordinates,
                normals: &[],
                colors: &[],
                texture_coordinates: &[],
            },
        )
    };
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(decode(ctx, std::slice::from_ref(&first)).unwrap().len(), 1);
    });
    let vertex_headers = [first.clone(), second.clone()];
    assert_released(|ctx| {
        assert!(decode(ctx, &vertex_headers)?.is_empty());
        Ok(())
    });
}

#[test]
fn compressed_sequence_candidate_releases_prefix_before_failed_inflation() {
    let mut expanded = framed_element([0; 16], 4, &[1]);
    end_marker(&mut expanded);
    let (mut container, first) = compressed_fixture(&expanded, 7);
    let mut second = first.clone();
    second.source_offset = cadmpeg_core::decode::u64_from_index(container.data.len());
    second.segment_byte_len = 37;
    container.data.to_mut().extend_from_slice(&[0; 37]);
    container.physical_size += 37;
    container.entries[0].body = crate::container::DirEntryBody::File {
        offset: 0,
        len: container.physical_size,
    };
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            display_jt_compressed_element_sequences(ctx, &container, std::slice::from_ref(&first))
                .unwrap()
                .1
                .len(),
            1
        );
    });
    assert_released(|ctx| {
        let (elements, sequences) = display_jt_compressed_element_sequences(
            ctx,
            &container,
            &[first.clone(), second.clone()],
        )?;
        assert!(elements.is_empty());
        assert!(sequences.is_empty());
        Ok(())
    });
}

fn string_body(text: &str) -> Vec<u8> {
    let mut bytes = vec![1, 0, 0, 0, 0, 0x40, 1, 0];
    let units: Vec<_> = text.encode_utf16().collect();
    bytes.extend_from_slice(&u32::try_from(units.len()).unwrap().to_le_bytes());
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

#[test]
fn string_property_candidate_releases_atom_before_wrong_type() {
    let mut expanded = framed_element(STRING_TYPE, 5, &string_body("x"));
    let mut valid = expanded.clone();
    end_marker(&mut valid);
    let (valid_container, valid_segment) = compressed_fixture(&valid, 31);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            display_jt_string_property_atoms(ctx, &valid_container, &[valid_segment])
                .unwrap()
                .len(),
            1
        );
    });
    expanded.extend_from_slice(&framed_element([0; 16], 5, &string_body("y")));
    end_marker(&mut expanded);
    let (container, segment) = compressed_fixture(&expanded, 31);
    assert_released(|ctx| {
        assert!(
            display_jt_string_property_atoms(ctx, &container, std::slice::from_ref(&segment))?
                .is_empty()
        );
        Ok(())
    });
}

fn topology_sequence() -> DisplayJtTopologyPacketSequence {
    let mut packets = Vec::new();
    let mut add = |role, values: &[i32]| {
        packets.push(DisplayJtTopologyPacket {
            role,
            value_count: u32::try_from(values.len()).unwrap(),
            codec: 0,
            byte_len: 0,
            sha256: Sha256Digest::digest(&[]),
            representation_offset: 0,
            values: Some(values.to_vec()),
        });
    };
    for context in TopologyContext::ALL {
        add(
            TopologyPacketRole::FaceDegrees(context),
            match u8::from(context) {
                0 => &[3, 3, 3],
                1 => &[3],
                _ => &[],
            },
        );
        add(
            TopologyPacketRole::FaceAttributeMasks(context),
            if u8::from(context) == 1 {
                &[1, 1, 1, 1]
            } else {
                &[]
            },
        );
    }
    add(TopologyPacketRole::VertexValences, &[3, 3, 3, 3]);
    add(TopologyPacketRole::VertexGroups, &[0, 0, 0, 0]);
    add(TopologyPacketRole::VertexFlags, &[0, 0, 0, 0]);
    add(TopologyPacketRole::FaceAttributeMasks7Next30, &[]);
    add(TopologyPacketRole::FaceAttributeMasks7Upper4, &[]);
    DisplayJtTopologyPacketSequence {
        id: "shape-topology-packets".into(),
        element: "shape".into(),
        packets,
        composite_hash: 0,
        topology_byte_len: 0,
        source_offset: 0,
    }
}

#[test]
fn polygon_mesh_candidate_releases_valid_mesh_before_missing_lane() {
    let first = topology_sequence();
    let mut second = first.clone();
    second.packets.clear();
    let mut header = coordinate_header();
    header.unique_vertex_count = 4;
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            display_jt_polygon_meshes(
                ctx,
                std::slice::from_ref(&first),
                std::slice::from_ref(&header)
            )
            .unwrap()
            .len(),
            1
        );
    });
    assert_released(|ctx| {
        assert!(display_jt_polygon_meshes(
            ctx,
            &[first.clone(), second.clone()],
            std::slice::from_ref(&header)
        )?
        .is_empty());
        Ok(())
    });
}

#[test]
fn binding_candidate_releases_valid_binding_before_trailing_property_bytes() {
    const LATE_TYPE: [u8; 16] = [
        0xe5, 0x5b, 0xb0, 0xe0, 0xbd, 0xfb, 0xd1, 0x11, 0xa3, 0xa7, 0, 0xaa, 0, 0xd1, 9, 0x54,
    ];
    let mut expanded = Vec::new();
    end_marker(&mut expanded);
    let mut late = 1_u16.to_le_bytes().to_vec();
    late.extend_from_slice(&0_u32.to_le_bytes());
    late.extend_from_slice(&1_u16.to_le_bytes());
    late.extend_from_slice(&[9; 16]);
    late.extend_from_slice(&7_u32.to_le_bytes());
    late.extend_from_slice(&12_u32.to_le_bytes());
    late.extend_from_slice(&1_u32.to_le_bytes());
    let mut late_atom = framed_element(LATE_TYPE, 8, &late);
    late_atom[21..25].copy_from_slice(&3_u32.to_le_bytes());
    expanded.extend_from_slice(&late_atom);
    let mut key_atom = framed_element(STRING_TYPE, 5, &string_body("JT_LLPROP_SHAPEIMPL"));
    key_atom[21..25].copy_from_slice(&4_u32.to_le_bytes());
    expanded.extend_from_slice(&key_atom);
    end_marker(&mut expanded);
    expanded.extend_from_slice(&1_u16.to_le_bytes());
    for value in [1_u32, 2, 4, 3, 0] {
        expanded.extend_from_slice(&value.to_le_bytes());
    }
    let target = DisplayJtSegment {
        segment_id: [9; 16],
        ..segment(&[], 7, None)
    };
    let (valid_container, valid_segment) = compressed_fixture(&expanded, 1);
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            display_jt_shape_lod_bindings(ctx, &valid_container, &[valid_segment, target.clone()])
                .unwrap()
                .len(),
            1
        );
    });
    expanded.push(1);
    let (container, source) = compressed_fixture(&expanded, 1);
    assert_released(|ctx| {
        assert!(
            display_jt_shape_lod_bindings(ctx, &container, &[source.clone(), target.clone()])?
                .is_empty()
        );
        Ok(())
    });
}

#[test]
fn scene_family_candidate_releases_prefix_when_its_next_member_fails() {
    let mut group = 1_u16.to_le_bytes().to_vec();
    group.extend_from_slice(&[0; 8]);
    group.extend_from_slice(&1_u16.to_le_bytes());
    group.extend_from_slice(&1_u32.to_le_bytes());
    group.extend_from_slice(&7_u32.to_le_bytes());
    let mut expanded = framed_element([0; 16], 1, &group);
    let mut valid = expanded.clone();
    end_marker(&mut valid);
    let (valid_container, valid_segment) = compressed_fixture(&valid, 1);
    let document = DisplayJtDocument {
        id: "document".into(),
        index_row: "row".into(),
        version: JtVersionField::new(format!("{:<80}", "Version 9.5"), Ok::<_, Infallible>)
            .unwrap()
            .unwrap(),
        toc_offset: 0,
        lsg_segment_id: [0; 16],
        toc_entries: vec![],
        physical_byte_len: 0,
        source_offset: 0,
    };
    crate::test_support::with_decode_context(|ctx| {
        let result = display_jt_scene_nodes(
            ctx,
            &valid_container,
            &[valid_segment],
            std::slice::from_ref(&document),
        )
        .unwrap();
        assert_eq!(result.base_nodes.len(), 1);
        assert_eq!(result.group_nodes.len(), 1);
    });
    expanded.extend_from_slice(&framed_element([0; 16], 1, &[]));
    end_marker(&mut expanded);
    let (container, segment) = compressed_fixture(&expanded, 1);
    assert_released(|ctx| {
        let result = display_jt_scene_nodes(
            ctx,
            &container,
            std::slice::from_ref(&segment),
            std::slice::from_ref(&document),
        )?;
        assert!(result.base_nodes.is_empty());
        assert!(result.group_nodes.is_empty());
        Ok(())
    });
}

#[test]
fn null_predictor_keeps_the_packet_without_copying_its_collection() {
    let mut packet = zero_packet();
    packet[..4].copy_from_slice(&2_u32.to_le_bytes());
    let (container, elements) = shape_fixture(&tri_strip_body(&packet));
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_collection_items = 3,
        |ctx| {
            let records =
                display_jt_initial_face_degree_symbols(ctx, &container, &elements[..1]).unwrap();
            assert_eq!(records[0].degrees, [0, 0]);
        },
    );
}

fn assert_first_visit_only(
    operation: &str,
    decode: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>,
) {
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        operation,
        &decode,
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("visit refusal");
    };
    assert_eq!(limit.additional, 1);
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = limit.used + 1,
        |ctx| decode(ctx).unwrap(),
    );
}

#[test]
fn range_nan_rejection_does_not_visit_the_remaining_values() {
    let mut bytes = 129_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&f32::NAN.to_le_bytes());
    bytes.extend_from_slice(&[0; 128 * 4]);
    assert_first_visit_only("decode DisplayJT range values", |ctx| {
        assert!(parse_jt_f32_vector(ctx, &mut View::over_retained(&bytes))?.is_none());
        Ok(())
    });
}

#[test]
fn negative_range_limit_rejection_does_not_visit_the_remaining_values() {
    let mut values = vec![FiniteBinary32::new(-1.).unwrap()];
    values.extend([FiniteBinary32::ZERO; 128]);
    assert_first_visit_only("validate DisplayJT range limits", |ctx| {
        assert!(
            JtRangeLimits::from_finite(values.clone(), &DecodeJtRangeLimitAdmission { ctx })?
                .is_err()
        );
        Ok(())
    });
}

#[test]
fn invalid_first_index_row_does_not_visit_the_remaining_rows() {
    let mut bytes = 9_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&129_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 129 * 16]);
    let container = container(bytes);
    assert_first_visit_only("scan DisplayJT index rows", |ctx| {
        assert!(display_jt_indices(ctx, &container)?.is_empty());
        Ok(())
    });
}

#[test]
fn invalid_first_toc_extent_does_not_visit_the_remaining_entries() {
    let mut container = super::document_admission_tests::one_document();
    let indices = crate::test_support::with_decode_context(|ctx| {
        display_jt_indices(ctx, &container).unwrap()
    });
    let data = container.data.to_mut();
    data[133..137].copy_from_slice(&129_u32.to_le_bytes());
    data[153..157].copy_from_slice(&u32::MAX.to_le_bytes());
    data.extend_from_slice(&[0; 128 * 28]);
    container.physical_size = u64::try_from(data.len()).unwrap();
    container.entries[0].body = crate::container::DirEntryBody::File {
        offset: 0,
        len: container.physical_size,
    };
    assert_first_visit_only("scan DisplayJT table of contents", |ctx| {
        assert!(display_jt_documents(ctx, &container, &indices)?.is_empty());
        Ok(())
    });
}

#[test]
fn topology_predictor_scratch_is_released_while_values_remain_retained() {
    const ALLOWANCE: u64 = 1 << 20;
    let mut representation = Vec::new();
    let mut packet = zero_packet();
    packet[..4].copy_from_slice(&2_u32.to_le_bytes());
    for ordinal in 0..24 {
        if [0, 10, 22].contains(&ordinal) {
            representation.extend_from_slice(&packet);
        } else {
            representation.extend_from_slice(&0_u32.to_le_bytes());
        }
    }
    representation.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
    representation.extend_from_slice(&[0; 16]);
    let (container, elements) = shape_fixture(&tri_strip_body(&representation));
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_retained_bytes = ALLOWANCE,
        |ctx| {
            let arrays =
                display_jt_topology_packet_sequences(ctx, &container, &elements[..1]).unwrap();
            assert_eq!(arrays.sequences.len(), 1);
            assert_eq!(arrays.vertex_headers.len(), 1);
            assert!(arrays.coordinate_headers.is_empty());
            let sequence = &arrays.sequences[0];
            for ordinal in [0, 10, 22] {
                assert_eq!(
                    sequence.packets[ordinal].values.as_deref(),
                    Some(&[0, 0][..])
                );
            }
            // Every SHA-256 field owns its exact 64-byte hexadecimal spelling.
            let mut retained = arrays.sequences.capacity()
                * std::mem::size_of::<DisplayJtTopologyPacketSequence>()
                + arrays.vertex_headers.capacity()
                    * std::mem::size_of::<DisplayJtCompressedVertexRecordsHeader>()
                + sequence.id.capacity()
                + sequence.element.capacity()
                + sequence.packets.capacity() * std::mem::size_of::<DisplayJtTopologyPacket>();
            for packet in &sequence.packets {
                retained += 64;
                if let Some(values) = &packet.values {
                    retained += values.capacity() * std::mem::size_of::<i32>();
                }
            }
            for header in &arrays.vertex_headers {
                retained += header.id.capacity() + header.element.capacity() + 64;
            }
            let remaining = usize::try_from(ALLOWANCE).unwrap() - retained;
            let probe = ctx
                .vector_storage::<u8>(remaining, "verify topology residual scratch release")
                .unwrap();
            assert_eq!(probe.capacity(), remaining);
        },
    );
}

fn assert_literal_admission(text: &'static str, operation: &'static str) {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = 0,
        |ctx| {
            assert_eq!(retain_jt_literal(ctx, text, operation).unwrap(), text);
            assert!(ctx.resource_refusal().is_none());
        },
    );
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        operation,
        |ctx| retain_jt_literal(ctx, text, operation),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == operation
            && limit.dimension == ResourceDimension::RetainedBytes
            && limit.additional == u64::try_from(text.len()).unwrap()));
}

#[test]
fn binding_key_literal_retains_storage_without_work_admission() {
    assert_literal_admission("JT_LLPROP_SHAPEIMPL", "retain DisplayJT binding key");
}

#[test]
fn sequence_error_literal_retains_storage_without_work_admission() {
    assert_literal_admission(
        "DisplayJtCompressedElementSequence.tail_sha256 disagrees with tail",
        "retain DisplayJT graph rejection",
    );
}

#[test]
fn sequence_arena_literal_retains_storage_without_work_admission() {
    assert_literal_admission(
        "display_jt_compressed_element_sequences",
        "retain native arena error name",
    );
}
