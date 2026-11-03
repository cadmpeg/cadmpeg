// SPDX-License-Identifier: Apache-2.0

use super::{Graph, NodeKind};
use crate::test_support::{test_bytes::encoded_xmt, with_decode_context};

const END: [u8; 4] = [0, 1, 0, 1];

fn header(version: u16, embedded: bool) -> Vec<u8> {
    let description = ": TRANSMIT FILE (partition) owned edges";
    let schema = if embedded {
        format!("SCH_999_999_{version}")
    } else {
        format!("SCH_999_{version}")
    };
    let mut bytes = b"PS".to_vec();
    for field in [description, schema.as_str()] {
        bytes.extend(u32::try_from(field.len()).unwrap().to_be_bytes());
        bytes.extend(field.as_bytes());
    }
    if embedded {
        bytes.extend(250u16.to_be_bytes());
    }
    bytes.extend(0u32.to_be_bytes());
    bytes
}

fn edge(first: bool, xmt: u32, node_id: u32, tolerance: f64) -> Vec<u8> {
    let mut bytes = 16u16.to_be_bytes().to_vec();
    if first {
        bytes.push(255);
    }
    bytes.extend(encoded_xmt(xmt));
    bytes.extend(node_id.to_be_bytes());
    bytes.extend(encoded_xmt(1));
    bytes.extend(tolerance.to_be_bytes());
    for reference in [45, 1, 46, 9, 1, 1, 3] {
        bytes.extend(encoded_xmt(reference));
    }
    bytes
}

fn vertex(xmt: u32, node_id: u32) -> Vec<u8> {
    let mut bytes = u16::from(NodeKind::Vertex.code()).to_be_bytes().to_vec();
    bytes.extend(encoded_xmt(xmt));
    bytes.extend(node_id.to_be_bytes());
    for reference in [1, 1, 1, 1, 45] {
        bytes.extend(encoded_xmt(reference));
    }
    bytes.extend(0.0f64.to_be_bytes());
    bytes.extend(encoded_xmt(3));
    bytes
}

fn parse(bytes: &[u8]) -> Graph {
    Graph::parse_source(bytes).unwrap()
}

#[test]
fn primary_edge_full_node_id_does_not_require_a_complete_graph() {
    with_decode_context(|_ctx| {
        for version in [11004, 13006] {
            for embedded in [false, true] {
                let mut bytes = header(version, embedded);
                let start = bytes.len();
                bytes.extend(edge(embedded, 11, u32::MAX, 0.0));
                let end = bytes.len();
                bytes.extend(END);
                let graph = parse(&bytes);
                let edge = graph.get(NodeKind::Edge, 11).expect("owned EDGE");
                assert_eq!(
                    (edge.pos(), edge.end, edge.node_id()),
                    (start, end, Some(u32::MAX))
                );
            }
        }
    });
}

#[test]
fn primary_edge_inside_an_array_is_not_a_node() {
    with_decode_context(|_ctx| {
        let fake = edge(false, 11, 73, 0.0);
        let mut bytes = header(13006, false);
        bytes.extend(96u16.to_be_bytes());
        bytes.extend(u32::try_from(fake.len() / 2).unwrap().to_be_bytes());
        bytes.extend(encoded_xmt(50));
        bytes.extend(fake);
        bytes.extend(END);
        let graph = parse(&bytes);
        assert_eq!(graph.kind_count(NodeKind::Edge), 0);
        assert!(graph.primary_edge_refusals().is_empty());
    });
}

#[test]
fn candidate_inside_a_framed_record_does_not_invalidate_the_framed_node() {
    with_decode_context(|_ctx| {
        let fake = vertex(82, 74);
        let mut bytes = header(13006, false);
        let start = bytes.len();
        let real = vertex(82, 73);
        let end = start + real.len();
        bytes.extend(real);
        bytes.extend(96u16.to_be_bytes());
        bytes.extend(u32::try_from(fake.len() / 2).unwrap().to_be_bytes());
        bytes.extend(encoded_xmt(50));
        bytes.extend(fake);
        bytes.extend(END);
        let graph = parse(&bytes);
        let vertex = graph.get(NodeKind::Vertex, 82).expect("framed VERTEX");
        assert_eq!(
            (vertex.pos(), vertex.end, vertex.node_id()),
            (start, end, Some(73))
        );
        assert_eq!(graph.kind_count(NodeKind::Vertex), 1);
    });
}

#[test]
fn primary_edge_stopped_or_terminated_cursor_never_resumes_scanning() {
    with_decode_context(|_ctx| {
        for suffix in [vec![0, 0], END.to_vec()] {
            let mut bytes = header(13006, false);
            bytes.extend(suffix);
            bytes.extend(edge(false, 11, 73, 0.0));
            assert_eq!(parse(&bytes).kind_count(NodeKind::Edge), 0);
        }
    });
}

#[test]
fn primary_edge_first_use_and_extended_identity_are_independent() {
    with_decode_context(|_ctx| {
        let mut bytes = header(13006, true);
        bytes.extend(edge(true, 11, 73, 0.0));
        let start = bytes.len();
        let identity = 32_767 + 255;
        bytes.extend(edge(false, identity, 74, 0.0));
        bytes.extend(END);
        let graph = parse(&bytes);
        assert_eq!(graph.kind_count(NodeKind::Edge), 2);
        assert_eq!(graph.get(NodeKind::Edge, identity).unwrap().pos(), start);
    });
}

#[test]
fn primary_edge_duplicate_identity_with_invalid_scalar_denies_both() {
    with_decode_context(|_ctx| {
        let mut bytes = header(13006, false);
        bytes.extend(edge(false, 11, 73, 0.0));
        bytes.extend(edge(false, 11, 74, f64::NAN));
        bytes.extend(END);
        assert_eq!(parse(&bytes).kind_count(NodeKind::Edge), 0);
    });
}

#[test]
fn primary_edge_complete_prefix_survives_a_later_stop() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let mut bytes = header(13006, false);
    bytes.extend(edge(false, 11, 73, 0.0));
    bytes.extend([0, 0]);
    bytes.extend(edge(false, 12, 74, 0.0));
    with_decode_context(|_ctx| {
        let graph = parse(&bytes);
        assert_eq!(graph.kind_count(NodeKind::Edge), 1);
        assert!(graph.get(NodeKind::Edge, 11).is_some());
        assert!(graph.get(NodeKind::Edge, 12).is_none());
    });
    let file = crate::test_support::test_prt::prt_with_partition(&bytes);
    let decoded = crate::NxCodec
        .decode(&mut std::io::Cursor::new(file), &DecodeOptions::default())
        .expect("retained stopped primary stream");
    let code = crate::loss::NxLossCode::PrimaryEdgeUnresolved.kind();
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == code)
            .count(),
        1
    );
}

#[test]
fn primary_stop_with_no_unread_bytes_or_a_terminator_reports_no_unread_region() {
    with_decode_context(|_ctx| {
        let mut exhausted = header(13006, false);
        exhausted.extend(edge(false, 11, 73, 0.0));
        assert_eq!(parse(&exhausted).primary_unread_from(), None);
        let mut terminated = exhausted.clone();
        terminated.extend(END);
        terminated.extend([0, 0, 7]);
        assert_eq!(parse(&terminated).primary_unread_from(), None);
        let mut stopped = exhausted.clone();
        let offset = stopped.len();
        stopped.extend([0, 0]);
        assert_eq!(parse(&stopped).primary_unread_from(), Some(offset));
    });
}

#[test]
fn primary_edge_unsupported_header_preserves_the_legacy_path() {
    with_decode_context(|_ctx| {
        let bytes = edge(false, 11, 73, 0.0);
        assert_eq!(parse(&bytes).kind_count(NodeKind::Edge), 1);
    });
}

#[test]
fn primary_edge_complete_duplicate_records_report_a_scoped_refusal() {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let mut bytes = header(13006, false);
    let first = bytes.len();
    bytes.extend(edge(false, 11, 73, 0.0));
    let second = bytes.len();
    bytes.extend(edge(false, 11, 74, f64::NAN));
    bytes.extend(END);
    with_decode_context(|_ctx| {
        assert_eq!(parse(&bytes).primary_edge_refusals(), [first, second]);
    });
    let file = crate::test_support::test_prt::prt_with_partition(&bytes);
    let decoded = crate::NxCodec
        .decode(&mut std::io::Cursor::new(file), &DecodeOptions::default())
        .expect("retained duplicate EDGE Source");
    let code = crate::loss::NxLossCode::PrimaryEdgeUnresolved.kind();
    assert_eq!(
        decoded
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == code)
            .count(),
        2
    );
}

/// A base-schema type-38 record with five construction references.
fn intersection(xmt: u32, supports: [u32; 2]) -> Vec<u8> {
    let mut bytes = u16::from(NodeKind::Intersection.code())
        .to_be_bytes()
        .to_vec();
    bytes.extend(encoded_xmt(xmt));
    bytes.extend(7u32.to_be_bytes());
    for reference in [1, 1, 1, 1, 1] {
        bytes.extend(encoded_xmt(reference));
    }
    bytes.push(b'+');
    for reference in [supports[0], supports[1], 1, 1, 1] {
        bytes.extend(encoded_xmt(reference));
    }
    bytes
}

fn point(xmt: u32) -> Vec<u8> {
    let mut bytes = u16::from(NodeKind::Point.code()).to_be_bytes().to_vec();
    bytes.extend(encoded_xmt(xmt));
    bytes.extend(9u32.to_be_bytes());
    for reference in [1, 1, 1, 1] {
        bytes.extend(encoded_xmt(reference));
    }
    for value in [0.0f64; 3] {
        bytes.extend(value.to_be_bytes());
    }
    bytes
}

#[test]
fn a_terminated_type_38_record_without_intersection_data_ends_at_its_fifth_reference() {
    with_decode_context(|_ctx| {
        let mut bytes = header(13006, false);
        let start = bytes.len();
        bytes.extend(intersection(3, [20, 21]));
        let end = bytes.len();
        bytes.extend(point(5));
        bytes.extend(END);
        let graph = parse(&bytes);
        let node = graph.get(NodeKind::Intersection, 3).expect("type-38 node");
        assert_eq!((node.pos(), node.end), (start, end));
        let curves = graph.composite_curves();
        assert_eq!(curves.len(), 1);
        assert!(curves[0].references[2..].iter().all(Option::is_none));
        assert!(graph.get(NodeKind::Point, 5).is_some());
    });
}

#[test]
fn a_type_38_record_without_intersection_data_before_an_edge_keeps_its_node() {
    with_decode_context(|_ctx| {
        let mut bytes = header(13006, true);
        bytes.extend(intersection_first_use(3, [20, 21]));
        let end = bytes.len();
        bytes.extend(edge(true, 11, 7, 0.0));
        bytes.extend(END);
        let graph = parse(&bytes);
        let node = graph.get(NodeKind::Intersection, 3).expect("type-38 node");
        assert_eq!(node.end, end);
        assert_eq!(graph.composite_curves().len(), 1);
        assert!(graph.get(NodeKind::Edge, 11).is_some());
    });
}

/// [`intersection`] with the first-use `ff` of an embedded-schema stream.
fn intersection_first_use(xmt: u32, supports: [u32; 2]) -> Vec<u8> {
    let mut bytes = intersection(xmt, supports);
    bytes.insert(2, 255);
    bytes
}
