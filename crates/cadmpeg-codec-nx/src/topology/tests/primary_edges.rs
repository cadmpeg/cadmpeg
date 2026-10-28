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
    with_decode_context(|_ctx| {
        let mut bytes = header(13006, false);
        bytes.extend(edge(false, 11, 73, 0.0));
        bytes.extend([0, 0]);
        bytes.extend(edge(false, 12, 74, 0.0));
        let graph = parse(&bytes);
        assert_eq!(graph.kind_count(NodeKind::Edge), 1);
        assert!(graph.get(NodeKind::Edge, 11).is_some());
        assert!(graph.get(NodeKind::Edge, 12).is_none());
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
