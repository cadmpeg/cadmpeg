// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use cadmpeg_core::decode::{DecodeContext, ResourceDimension};
use cadmpeg_core::CodecError;

const TRI_STRIP_LOD_TYPE: [u8; 16] = [
    0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
    0x97,
];

fn completes_with_work(
    run: impl for<'ctx> Fn(&DecodeContext<'ctx>) -> Result<bool, CodecError>,
    cap: u64,
) -> bool {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_work_units = cap,
        |ctx| matches!(run(ctx), Ok(true)) && ctx.resource_refusal().is_none(),
    )
}

fn minimum_work(
    run: impl for<'ctx> Fn(&DecodeContext<'ctx>) -> Result<bool, CodecError>,
) -> u64 {
    let mut high = 1_u64;
    while !completes_with_work(&run, high) {
        high = high.checked_mul(2).expect("JT test work bound fits u64");
    }
    let mut low = 0_u64;
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if completes_with_work(&run, middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

fn assert_charged_visit(
    operation: &'static str,
    run: impl for<'ctx> Fn(&DecodeContext<'ctx>) -> Result<bool, CodecError>,
) {
    let CodecError::ResourceLimit(limit) = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        operation,
        |ctx| run(ctx),
    )
    else {
        panic!("the first charged visit must refuse");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.additional, 1);

    crate::test_support::with_decode_context(|ctx| {
        assert!(run(ctx).expect("the first invalid prefix returns its semantic result"));
        assert!(ctx.resource_refusal().is_none());
    });
}

fn assert_prefix_without_suffix(
    operation: &'static str,
    short: impl for<'ctx> Fn(&DecodeContext<'ctx>) -> Result<bool, CodecError>,
    long: impl for<'ctx> Fn(&DecodeContext<'ctx>) -> Result<bool, CodecError>,
) {
    assert_charged_visit(operation, &long);

    let short_need = minimum_work(&short);
    let long_need = minimum_work(&long);
    assert_eq!(long_need, short_need, "the suffix must remain unvisited");
    assert!(completes_with_work(&long, long_need));
    assert!(!completes_with_work(&long, long_need - 1));
}

fn container(data: Vec<u8>) -> Container<'static> {
    let physical_size = cadmpeg_core::decode::u64_from_index(data.len());
    Container {
        data: data.into(),
        physical_size,
        layout: crate::container::test_modern_layout(6),
        entries: vec![crate::container::DirEntry {
            name: "/Root/UG_PART/DisplayJT".into(),
            region: crate::container::Region::Footer,
            body: crate::container::DirEntryBody::File {
                offset: 0,
                len: physical_size,
            },
        }],
        fastload_table: None,
        segment_index: None,
        segment_wrappers: Vec::new(),
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    }
}

fn empty_container() -> Container<'static> {
    container(Vec::new())
}

fn malformed_shape_element() -> DisplayJtShapeLodElement {
    DisplayJtShapeLodElement {
        id: "shape-element".into(),
        segment: "shape-segment".into(),
        ordinal: 0,
        object_type_id: TRI_STRIP_LOD_TYPE,
        object_id: 7,
        body_byte_len: 0,
        body_sha256: Sha256Digest::digest(&[]),
        source_offset: u64::MAX,
    }
}

fn malformed_coordinate_header() -> DisplayJtVertexCoordinateArrayHeader {
    DisplayJtVertexCoordinateArrayHeader {
        id: "coordinate-header".into(),
        element: "element".into(),
        unique_vertex_count: 1,
        component_count: 3,
        component_ranges: [QuantizedRange::ZERO; 3],
        component_quantization_bits: [0; 3],
        compressed_components_byte_len: 0,
        compressed_components_sha256: Sha256Digest::digest(&[]),
        source_offset: u64::MAX,
    }
}

fn incomplete_vertex_header(vertex_bindings: u64) -> DisplayJtCompressedVertexRecordsHeader {
    DisplayJtCompressedVertexRecordsHeader {
        id: "vertex-header".into(),
        element: "element".into(),
        vertex_bindings,
        vertex_quantization_bits: 0,
        normal_quantization_factor: 0,
        texture_quantization_bits: 0,
        color_quantization_bits: 0,
        topological_vertex_count: 1,
        vertex_attribute_count: 1,
        compressed_arrays_byte_len: 0,
        compressed_arrays_sha256: Sha256Digest::digest(&[]),
        source_offset: 0,
    }
}

fn segment(segment_type: u32, compression: Option<DisplayJtCompression>) -> DisplayJtSegment {
    DisplayJtSegment {
        id: "segment".into(),
        document: "document".into(),
        toc_entry: "toc-entry".into(),
        segment_id: [0; 16],
        segment_type,
        segment_byte_len: 0,
        payload_sha256: Sha256Digest::digest(&[]),
        compression,
        source_offset: 0,
    }
}

fn invalid_compression() -> DisplayJtCompression {
    DisplayJtCompression {
        envelope: JtCompressionEnvelope {
            compressed_byte_len: 0,
        },
        inflated_sha256: Sha256Digest::digest(&[]),
    }
}

fn tessellation_inputs(meshes: &[DisplayJtPolygonMesh]) -> DisplayJtTessellationInputs<'_> {
    DisplayJtTessellationInputs {
        meshes,
        coordinates: &[],
        normals: &[],
        colors: &[],
        texture_coordinates: &[],
        vertex_flags: &[],
        vertex_headers: &[],
        coordinate_headers: &[],
        shape_elements: &[],
        bindings: &[],
        shape_nodes: &[],
        base_nodes: &[],
        group_nodes: &[],
        instance_nodes: &[],
        transforms: &[],
        materials: &[],
        compressed_elements: &[],
    }
}

fn base_node(id: String, object_id: u32) -> DisplayJtBaseNodeData {
    DisplayJtBaseNodeData {
        id,
        element: "element".into(),
        object_type_id: [0; 16],
        object_id,
        version: 1,
        flags: 0,
        attribute_object_ids: Vec::new(),
        family_data_byte_len: 0,
        family_data_sha256: Sha256Digest::digest(&[]),
        source_offset: 0,
    }
}

fn compressed_element() -> DisplayJtCompressedElement {
    DisplayJtCompressedElement {
        id: "element".into(),
        segment: "scene".into(),
        segment_type: 1,
        ordinal: 0,
        object_type_id: [0; 16],
        object_base_type: 0,
        object_id: 7,
        body_byte_len: 0,
        body_sha256: Sha256Digest::digest(&[]),
        inflated_offset: 0,
        source_offset: 0,
    }
}

fn topology_packet(
    role: TopologyPacketRole,
    values: Option<Vec<i32>>,
) -> DisplayJtTopologyPacket {
    DisplayJtTopologyPacket {
        role,
        value_count: values
            .as_ref()
            .map_or(0, |values| u32::try_from(values.len()).expect("fixture count fits u32")),
        codec: 0,
        byte_len: 4,
        sha256: Sha256Digest::digest(&[]),
        representation_offset: 0,
        values,
    }
}

fn topology_packet_sequence(packets: Vec<DisplayJtTopologyPacket>) -> DisplayJtTopologyPacketSequence {
    DisplayJtTopologyPacketSequence {
        id: "sequence".into(),
        element: "element".into(),
        packets,
        composite_hash: 0,
        topology_byte_len: 0,
        source_offset: 0,
    }
}

#[test]
fn face_degree_and_topology_scans_stop_at_the_first_bad_shape_element() {
    let container = empty_container();
    let short_elements = vec![malformed_shape_element()];
    let long_elements = vec![malformed_shape_element(); 129];
    let short_face = |ctx: &DecodeContext<'_>| {
        Ok(display_jt_initial_face_degree_symbols(ctx, &container, &short_elements)?.is_empty())
    };
    let long_face = |ctx: &DecodeContext<'_>| {
        Ok(display_jt_initial_face_degree_symbols(ctx, &container, &long_elements)?.is_empty())
    };
    assert_prefix_without_suffix("scan DisplayJT face degree packets", short_face, long_face);

    let short_topology = |ctx: &DecodeContext<'_>| {
        Ok(display_jt_topology_packet_sequences(ctx, &container, &short_elements)?
            .sequences
            .is_empty())
    };
    let long_topology = |ctx: &DecodeContext<'_>| {
        Ok(display_jt_topology_packet_sequences(ctx, &container, &long_elements)?
            .sequences
            .is_empty())
    };
    assert_prefix_without_suffix(
        "scan DisplayJT topology packets",
        short_topology,
        long_topology,
    );
}

#[test]
fn topology_role_packets_receive_individual_work_admission() {
    let mut representation = vec![0; 24 * 4];
    representation.extend_from_slice(&0_u32.to_le_bytes());
    representation.extend_from_slice(&0_u64.to_le_bytes());
    representation.extend_from_slice(&[0; 4]);
    representation.extend_from_slice(&0_u32.to_le_bytes());
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u64.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&representation);
    let mut data = vec![0; 25];
    data.extend_from_slice(&body);
    let container = container(data);
    let elements = [DisplayJtShapeLodElement {
        id: "shape-element".into(),
        segment: "shape-segment".into(),
        ordinal: 0,
        object_type_id: TRI_STRIP_LOD_TYPE,
        object_id: 7,
        body_byte_len: u32::try_from(body.len()).expect("fixture length fits u32"),
        body_sha256: Sha256Digest::digest(&body),
        source_offset: 0,
    }];
    assert_charged_visit("nx JT topology packets", |ctx: &DecodeContext<'_>| {
        let sequences = display_jt_topology_packet_sequences(ctx, &container, &elements)?.sequences;
        Ok(sequences.len() == 1 && sequences[0].packets.len() == 24)
    });
}

#[test]
fn coordinate_mesh_and_vertex_array_scans_stop_at_the_first_bad_record() {
    let container = empty_container();
    let short_coordinate_headers = vec![malformed_coordinate_header()];
    let long_coordinate_headers = vec![malformed_coordinate_header(); 129];
    let short_coordinates = |ctx: &DecodeContext<'_>| {
        Ok(display_jt_vertex_coordinates(ctx, &container, &short_coordinate_headers)?.is_empty())
    };
    let long_coordinates = |ctx: &DecodeContext<'_>| {
        Ok(display_jt_vertex_coordinates(ctx, &container, &long_coordinate_headers)?.is_empty())
    };
    assert_prefix_without_suffix(
        "scan DisplayJT coordinate headers",
        short_coordinates,
        long_coordinates,
    );

    let sequence = DisplayJtTopologyPacketSequence {
        id: "sequence".into(),
        element: "element".into(),
        packets: Vec::new(),
        composite_hash: 0,
        topology_byte_len: 0,
        source_offset: 0,
    };
    let short_sequences = vec![sequence.clone()];
    let long_sequences = vec![sequence; 129];
    let short_meshes = |ctx: &DecodeContext<'_>| {
        Ok(display_jt_polygon_meshes(ctx, &short_sequences, &[])?.is_empty())
    };
    let long_meshes = |ctx: &DecodeContext<'_>| {
        Ok(display_jt_polygon_meshes(ctx, &long_sequences, &[])?.is_empty())
    };
    assert_prefix_without_suffix(
        "scan DisplayJT topology sequences",
        short_meshes,
        long_meshes,
    );

    for (bindings, operation, decode) in [
        (0x8_u64, "scan DisplayJT normal headers", 0_u8),
        (0x10_u64, "scan DisplayJT color headers", 1_u8),
        (1_u64 << 8, "scan DisplayJT texture headers", 2_u8),
        (0x40_u64, "scan DisplayJT flag headers", 3_u8),
    ] {
        let short_vertex_headers = vec![incomplete_vertex_header(bindings)];
        let long_vertex_headers = vec![incomplete_vertex_header(bindings); 129];
        let run = |ctx: &DecodeContext<'_>, vertex_headers: &[DisplayJtCompressedVertexRecordsHeader]| {
            let coordinate_headers: &[DisplayJtVertexCoordinateArrayHeader] = &[];
            let coordinates: &[DisplayJtVertexCoordinates] = &[];
            let normals: &[DisplayJtVertexNormals] = &[];
            let colors: &[DisplayJtVertexColors] = &[];
            let texture_coordinates: &[DisplayJtVertexTextureCoordinates] = &[];
            let arrays_are_empty = match decode {
                0 => display_jt_vertex_normals(
                    ctx,
                    &container,
                    &vertex_headers,
                    coordinate_headers,
                    coordinates,
                )?
                .is_empty(),
                1 => display_jt_vertex_colors(
                    ctx,
                    &container,
                    &vertex_headers,
                    coordinate_headers,
                    coordinates,
                    normals,
                )?
                .is_empty(),
                2 => display_jt_vertex_texture_coordinates(
                    ctx,
                    &container,
                    &vertex_headers,
                    coordinate_headers,
                    coordinates,
                    normals,
                    colors,
                )?
                .is_empty(),
                _ => display_jt_vertex_flags(
                    ctx,
                    DisplayJtVertexFlagInputs {
                        container: &container,
                        vertex_headers,
                        coordinate_headers,
                        coordinates,
                        normals,
                        colors,
                        texture_coordinates,
                    },
                )?
                .is_empty(),
            };
            Ok(arrays_are_empty)
        };
        let short = |ctx: &DecodeContext<'_>| run(ctx, &short_vertex_headers);
        let long = |ctx: &DecodeContext<'_>| run(ctx, &long_vertex_headers);
        assert_prefix_without_suffix(operation, short, long);
    }
}

#[test]
fn high_degree_mask_scan_stops_at_the_first_missing_lane_value() {
    let mut prefix = Vec::new();
    for context in TopologyContext::ALL {
        prefix.push(topology_packet(
            TopologyPacketRole::FaceDegrees(context),
            Some(Vec::new()),
        ));
    }
    prefix.push(topology_packet(
        TopologyPacketRole::VertexValences,
        Some(vec![1]),
    ));
    prefix.push(topology_packet(
        TopologyPacketRole::VertexGroups,
        Some(Vec::new()),
    ));
    prefix.push(topology_packet(
        TopologyPacketRole::VertexFlags,
        Some(Vec::new()),
    ));
    for context in TopologyContext::ALL {
        prefix.push(topology_packet(
            TopologyPacketRole::FaceAttributeMasks(context),
            Some(Vec::new()),
        ));
    }
    prefix.push(topology_packet(
        TopologyPacketRole::FaceAttributeMasks7Next30,
        Some(Vec::new()),
    ));
    prefix.push(topology_packet(
        TopologyPacketRole::FaceAttributeMasks7Upper4,
        Some(Vec::new()),
    ));
    prefix.push(topology_packet(
        TopologyPacketRole::HighDegreeFaceAttributeMasks(0),
        None,
    ));

    let suffix_packet = topology_packet(
        TopologyPacketRole::VertexGroups,
        Some(Vec::new()),
    );
    let short_packets = prefix.clone();
    let mut long_packets = prefix;
    long_packets.extend(std::iter::repeat(suffix_packet).take(128));
    let short_sequences = [topology_packet_sequence(short_packets)];
    let long_sequences = [topology_packet_sequence(long_packets)];
    let coordinate_headers = [malformed_coordinate_header()];
    let run = |ctx: &DecodeContext<'_>, sequences: &[DisplayJtTopologyPacketSequence]| {
        Ok(display_jt_polygon_meshes(ctx, sequences, &coordinate_headers)?.is_empty())
    };
    assert_prefix_without_suffix(
        "nx JT large mask lanes",
        |ctx: &DecodeContext<'_>| run(ctx, &short_sequences),
        |ctx: &DecodeContext<'_>| run(ctx, &long_sequences),
    );
}

#[test]
fn segment_and_scene_scans_stop_at_the_first_invalid_segment() {
    let container = empty_container();
    let short_compressed = vec![segment(2, Some(invalid_compression()))];
    let long_compressed = vec![segment(2, Some(invalid_compression())); 129];
    let short_compressed_run = |ctx: &DecodeContext<'_>| {
        let (elements, sequences) =
            display_jt_compressed_element_sequences(ctx, &container, &short_compressed)?;
        Ok(elements.is_empty() && sequences.is_empty())
    };
    let long_compressed_run = |ctx: &DecodeContext<'_>| {
        let (elements, sequences) =
            display_jt_compressed_element_sequences(ctx, &container, &long_compressed)?;
        Ok(elements.is_empty() && sequences.is_empty())
    };
    assert_prefix_without_suffix(
        "scan DisplayJT compressed segments",
        short_compressed_run,
        long_compressed_run,
    );

    let short_property_segments = vec![segment(31, None)];
    let long_property_segments = vec![segment(31, None); 129];
    let short_properties = |ctx: &DecodeContext<'_>| {
        Ok(display_jt_string_property_atoms(ctx, &container, &short_property_segments)?.is_empty())
    };
    let long_properties = |ctx: &DecodeContext<'_>| {
        Ok(display_jt_string_property_atoms(ctx, &container, &long_property_segments)?.is_empty())
    };
    assert_prefix_without_suffix(
        "scan DisplayJT property segments",
        short_properties,
        long_properties,
    );

    let long_binding_segments = vec![segment(1, None); 129];
    let long_bindings = |ctx: &DecodeContext<'_>| {
        Ok(display_jt_shape_lod_bindings(ctx, &container, &long_binding_segments)?.is_empty())
    };
    assert_charged_visit("scan DisplayJT binding segments", long_bindings);

    let documents: &[DisplayJtDocument] = &[];
    let short_scene_segments = vec![segment(1, None)];
    let long_scene_segments = vec![segment(1, None); 129];
    let run_scene = |ctx: &DecodeContext<'_>, scene_segments: &[DisplayJtSegment]| {
        let nodes = display_jt_scene_nodes(ctx, &container, scene_segments, documents)?;
        Ok(nodes.base_nodes.is_empty()
            && nodes.group_nodes.is_empty()
            && nodes.instance_nodes.is_empty()
            && nodes.transforms.is_empty()
            && nodes.materials.is_empty()
            && nodes.partition_nodes.is_empty()
            && nodes.range_lod_nodes.is_empty()
            && nodes.tri_strip_shape_nodes.is_empty())
    };
    assert_prefix_without_suffix(
        "scan DisplayJT scene segments",
        |ctx: &DecodeContext<'_>| run_scene(ctx, &short_scene_segments),
        |ctx: &DecodeContext<'_>| run_scene(ctx, &long_scene_segments),
    );
}

#[test]
fn tessellation_scan_stops_at_the_first_mesh_without_its_coordinate_header() {
    let short_meshes = vec![
        DisplayJtPolygonMesh {
            id: "mesh".into(),
            topology: "sequence".into(),
            coordinate_header: "missing".into(),
            polygons: Vec::new(),
            source_offset: 0,
        }
    ];
    let long_meshes = vec![short_meshes[0].clone(); 129];
    let short_inputs = tessellation_inputs(&short_meshes);
    let long_inputs = tessellation_inputs(&long_meshes);
    assert_prefix_without_suffix(
        "nx JT tessellation meshes",
        |ctx: &DecodeContext<'_>| Ok(display_jt_tessellations(ctx, &short_inputs)?.is_empty()),
        |ctx: &DecodeContext<'_>| Ok(display_jt_tessellations(ctx, &long_inputs)?.is_empty()),
    );
}

#[test]
fn scene_graph_base_scan_stops_at_the_first_duplicate_object_id() {
    let short_bases = vec![base_node("base-a".into(), 7), base_node("base-b".into(), 7)];
    let mut long_bases = short_bases.clone();
    long_bases.extend((0..127).map(|ordinal| {
        base_node(format!("base-suffix-{ordinal}"), 100 + ordinal)
    }));
    let compressed_elements = [compressed_element()];
    let index_inputs = DisplayJtTessellationInputs {
        meshes: &[],
        coordinates: &[],
        normals: &[],
        colors: &[],
        texture_coordinates: &[],
        vertex_flags: &[],
        vertex_headers: &[],
        coordinate_headers: &[],
        shape_elements: &[],
        bindings: &[],
        shape_nodes: &[],
        base_nodes: &[],
        group_nodes: &[],
        instance_nodes: &[],
        transforms: &[],
        materials: &[],
        compressed_elements: &compressed_elements,
    };
    let run = |ctx: &DecodeContext<'_>, bases: &[DisplayJtBaseNodeData]| {
        let inputs = DisplayJtTessellationInputs {
            base_nodes: bases,
            ..index_inputs
        };
        let index = JtTessellationIndex::new(ctx, &index_inputs)?;
        Ok(JtSceneGraph::new(ctx, "scene", &inputs, &index)?.is_none())
    };
    assert_prefix_without_suffix(
        "nx JT scoped base nodes",
        |ctx: &DecodeContext<'_>| run(ctx, &short_bases),
        |ctx: &DecodeContext<'_>| run(ctx, &long_bases),
    );
}
