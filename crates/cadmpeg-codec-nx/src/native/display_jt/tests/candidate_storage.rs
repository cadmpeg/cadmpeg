// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::hash::digest::Sha256Digest;

const INSTANCE_NODE_TYPE: [u8; 16] = [
    0x2a, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
    0x97,
];

struct SceneFixture {
    data: Vec<u8>,
    segment: DisplayJtSegment,
    document: DisplayJtDocument,
}

fn scene_element(type_id: [u8; 16], base_type: u8, object_id: u32, body: &[u8]) -> Vec<u8> {
    let mut element = Vec::new();
    element.extend_from_slice(
        &u32::try_from(21 + body.len())
            .expect("fixture element length fits u32")
            .to_le_bytes(),
    );
    element.extend_from_slice(&type_id);
    element.push(base_type);
    element.extend_from_slice(&object_id.to_le_bytes());
    element.extend_from_slice(body);
    element
}

fn scene_node_body(child_object_id: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&child_object_id.to_le_bytes());
    body
}

fn scene_fixture(first_type: [u8; 16], second_type: [u8; 16]) -> SceneFixture {
    let mut inflated = scene_element(first_type, 0, 7, &scene_node_body(8));
    inflated.extend_from_slice(&scene_element(second_type, 2, 9, &scene_node_body(10)));
    inflated.extend_from_slice(&16_u32.to_le_bytes());
    inflated.extend_from_slice(&[0xff; 16]);

    let compressed = crate::test_support::test_bytes::zlib_compress_at_level(&inflated, 1);
    let mut data = vec![0; 33];
    data.extend_from_slice(&compressed);
    let segment = DisplayJtSegment {
        id: "scene-segment".into(),
        document: "document".into(),
        toc_entry: "toc-entry".into(),
        segment_id: [0; 16],
        segment_type: 1,
        segment_byte_len: u32::try_from(data.len()).expect("fixture segment length fits u32"),
        payload_sha256: Sha256Digest::digest(&data[24..]),
        compression: Some(DisplayJtCompression {
            envelope: JtCompressionEnvelope {
                compressed_byte_len: u32::try_from(compressed.len())
                    .expect("fixture compressed length fits u32"),
            },
            inflated_sha256: Sha256Digest::digest(&inflated),
        }),
        source_offset: 0,
    };
    let document = DisplayJtDocument {
        id: "document".into(),
        index_row: "row".into(),
        version: JtVersionField::new(&format!("{:<80}", "Version 9.4 JT"))
            .expect("fixture version field is valid"),
        toc_offset: 0,
        lsg_segment_id: [0; 16],
        toc_entries: Vec::new(),
        physical_byte_len: cadmpeg_core::decode::u64_from_index(data.len()),
        source_offset: 0,
    };
    SceneFixture {
        data,
        segment,
        document,
    }
}

fn scene_container(fixture: &SceneFixture) -> Container<'_> {
    let physical_size = cadmpeg_core::decode::u64_from_index(fixture.data.len());
    Container {
        data: fixture.data.as_slice().into(),
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

fn decode_scene(
    fixture: &SceneFixture,
    max_retained_bytes: u64,
) -> Result<DisplayJtSceneNodes, CodecError> {
    let container = scene_container(fixture);
    crate::test_support::with_decode_context_over(
        &fixture.data,
        |policy: &mut DecodePolicy| policy.limits.max_retained_bytes = max_retained_bytes,
        |ctx: &DecodeContext<'_>| {
            display_jt_scene_nodes(
                ctx,
                &container,
                std::slice::from_ref(&fixture.segment),
                std::slice::from_ref(&fixture.document),
            )
        },
    )
}

fn accepted_base_node_retained_bytes(fixture: &SceneFixture) -> u64 {
    // Two records fit the first four-slot vector allocation. Each record owns
    // two formatted identities and one 64-byte digest; its attribute list is empty.
    let id_len = fixture.segment.id.len() + "-base-node-".len() + 1;
    let element_len = fixture.segment.id.len() + "-inflated-element-".len() + 1;
    cadmpeg_core::decode::u64_from_index(
        4 * std::mem::size_of::<DisplayJtBaseNodeData>() + 2 * (id_len + element_len + 64),
    )
}

#[test]
fn rejected_late_scene_family_releases_accepted_candidate_storage() {
    let baseline = scene_fixture([0x11; 16], [0x12; 16]);
    let late_rejection = scene_fixture(INSTANCE_NODE_TYPE, INSTANCE_NODE_TYPE);
    let retained_limit = accepted_base_node_retained_bytes(&baseline);

    let baseline_nodes = decode_scene(&baseline, retained_limit).expect("two base nodes fit");
    assert_eq!(baseline_nodes.base_nodes.len(), 2);
    let refusal = match decode_scene(&baseline, retained_limit - 1) {
        Ok(_) => panic!("one byte below the bound refuses"),
        Err(refusal) => refusal,
    };
    assert!(matches!(
        refusal,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "nx JT scene node candidates"
                && limit.additional > 0
    ));

    let nodes = decode_scene(&late_rejection, retained_limit)
        .expect("the rejected instance family releases its partial candidate");
    assert_eq!(
        nodes
            .base_nodes
            .iter()
            .map(|node| node.object_id)
            .collect::<Vec<_>>(),
        [7, 9]
    );
    assert!(nodes.instance_nodes.is_empty());
    assert!(nodes.group_nodes.is_empty());
    assert!(nodes.transforms.is_empty());
    assert!(nodes.materials.is_empty());
    assert!(nodes.partition_nodes.is_empty());
    assert!(nodes.range_lod_nodes.is_empty());
    assert!(nodes.tri_strip_shape_nodes.is_empty());

    // The baseline and late-rejection fixtures produce the same two base
    // records apart from each inline, fixed-size object-type identifier. The
    // retained limit is the source-derived accepted output bound, so the
    // invalid instance candidate must release its temporary storage before
    // the same-sized base-node output is committed.
    let mut baseline_records = baseline_nodes.base_nodes.clone();
    for (baseline, actual) in baseline_records.iter_mut().zip(&nodes.base_nodes) {
        baseline.object_type_id = actual.object_type_id;
    }
    assert_eq!(baseline_records, nodes.base_nodes);
}

#[test]
fn scene_element_visits_are_admitted_before_each_decode() {
    let fixture = scene_fixture([0x11; 16], [0x12; 16]);
    let container = scene_container(&fixture);
    let error = crate::test_support::resource_refusal_at(
        &fixture.data,
        ResourceDimension::WorkUnits,
        "decode DisplayJT scene elements",
        |ctx| {
            display_jt_scene_nodes(
                ctx,
                &container,
                std::slice::from_ref(&fixture.segment),
                std::slice::from_ref(&fixture.document),
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "decode DisplayJT scene elements"
                && limit.additional == 1
    ));
    let nodes = decode_scene(&fixture, u64::MAX).expect("the framed scene completes");
    assert_eq!(nodes.base_nodes.len(), 2);
}

fn shape_lod_fixture(representation: &[u8]) -> (Container<'static>, DisplayJtShapeLodElement) {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&10_u64.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(representation);
    let mut data = vec![0; 25];
    data.extend_from_slice(&body);
    let physical_size = cadmpeg_core::decode::u64_from_index(data.len());
    let container = Container {
        data: data.into(),
        physical_size,
        layout: crate::container::test_modern_layout(6),
        entries: vec![crate::container::DirEntry {
            name: "/Root/UG_PART/DisplayJT".into(),
            region: crate::container::Region::Header,
            body: crate::container::DirEntryBody::File { offset: 0, len: physical_size },
        }],
        fastload_table: None,
        segment_index: None,
        segment_wrappers: Vec::new(),
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let element = DisplayJtShapeLodElement {
        id: "shape-lod".into(),
        segment: "segment".into(),
        ordinal: 0,
        object_type_id: [
            0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb,
            0x59, 0x97,
        ],
        object_id: 1,
        body_byte_len: u32::try_from(body.len()).expect("fixture body fits u32"),
        body_sha256: Sha256Digest::digest(&body),
        source_offset: 0,
    };
    (container, element)
}

// Fixed-width mode, range -1..=1, followed by the codes for 1 and -1.
const TWO_SIGNED_VALUES: [u8; 13] = [2, 0, 0, 0, 1, 21, 0, 0, 0, 0x00, 0xc0, 0x16, 0x04];

#[test]
fn initial_face_degrees_retain_only_the_null_predictor_output() {
    let (container, element) = shape_lod_fixture(&TWO_SIGNED_VALUES);
    // One four-slot record vector, two integers, two identity strings, and a digest.
    let retained = cadmpeg_core::decode::u64_from_index(
        4 * std::mem::size_of::<DisplayJtInitialFaceDegreeSymbols>()
            + 2 * std::mem::size_of::<i32>()
            + 2 * element.id.len() + "-initial-face-degrees".len() + 64,
    );
    for cap in [retained - 1, retained] {
        crate::test_support::with_decode_context_over(
            container.data.as_ref(),
            |policy| policy.limits.max_retained_bytes = cap,
            |ctx| {
                let result = display_jt_initial_face_degree_symbols(
                    ctx, &container, std::slice::from_ref(&element),
                );
                if cap == retained {
                    let records = result.expect("the final output fits without a duplicate vector");
                    assert_eq!(records.len(), 1);
                    assert_eq!(records[0].degrees, [1, -1]);
                    assert_eq!(records[0].packet_byte_len, 13);
                    assert!(ctx.resource_refusal().is_none());
                } else {
                    let Err(CodecError::ResourceLimit(limit)) = result else {
                        panic!("one byte below final storage must refuse");
                    };
                    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                    assert_eq!(limit.operation, "nx JT face degree candidates");
                    assert_eq!(limit.additional, retained);
                    assert_eq!(ctx.resource_refusal(), Some(limit));
                }
            },
        );
    }
}

#[test]
fn topology_packets_release_lag_predictor_residual_storage() {
    let mut representation = Vec::new();
    // Eight degree lanes, valences, groups, then Lag1 vertex flags; the
    // remaining empty lanes complete the 24-packet topology envelope.
    for ordinal in 0..24 {
        representation.extend_from_slice(if ordinal == 10 { &TWO_SIGNED_VALUES } else { &[0; 4] });
    }
    representation.extend_from_slice(&0x1234_5678_u32.to_le_bytes());
    representation.extend_from_slice(&10_u64.to_le_bytes());
    representation.extend_from_slice(&[24, 13, 16, 8]);
    representation.extend_from_slice(&0_u32.to_le_bytes());
    let (container, element) = shape_lod_fixture(&representation);
    // Exact 24-slot packet vector, four-slot sequence and vertex-header vectors,
    // one two-integer final lane, 25 digests, and four owner/id strings.
    let retained = cadmpeg_core::decode::u64_from_index(
        24 * std::mem::size_of::<DisplayJtTopologyPacket>()
            + 4 * std::mem::size_of::<DisplayJtTopologyPacketSequence>()
            + 4 * std::mem::size_of::<DisplayJtCompressedVertexRecordsHeader>()
            + 2 * std::mem::size_of::<i32>() + 25 * 64
            + 4 * element.id.len() + "-topology-packets".len() + "-vertex-records-header".len(),
    );
    for cap in [retained - 1, retained] {
        crate::test_support::with_decode_context_over(
            container.data.as_ref(),
            |policy| policy.limits.max_retained_bytes = cap,
            |ctx| {
                let result = display_jt_topology_packet_sequences(
                    ctx, &container, std::slice::from_ref(&element),
                );
                if cap == retained {
                    let arrays = result.expect("only final lane values remain retained");
                    assert_eq!(arrays.sequences.len(), 1);
                    assert_eq!(arrays.vertex_headers.len(), 1);
                    assert!(arrays.coordinate_headers.is_empty());
                    assert_eq!(arrays.sequences[0].packets[10].values.as_deref(), Some([1, -1].as_slice()));
                    assert!(ctx.resource_refusal().is_none());
                } else {
                    let Err(CodecError::ResourceLimit(limit)) = result else {
                        panic!("one byte below final storage must refuse");
                    };
                    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
                    assert_eq!(limit.operation, "nx JT topology output candidates");
                    assert_eq!(limit.additional, retained);
                    assert_eq!(ctx.resource_refusal(), Some(limit));
                }
            },
        );
    }
}
