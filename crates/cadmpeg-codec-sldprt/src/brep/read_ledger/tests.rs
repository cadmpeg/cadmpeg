// SPDX-License-Identifier: Apache-2.0

use super::node_spans;
use crate::brep::topology::nodes::{sequence, tables};
use crate::test_support::parasolid::sequence::{sequence_of, topology_sequence, F};

#[test]
fn a_node_read_stream_names_each_admitted_node_over_its_range() {
    let (payload, body_offset) = topology_sequence();
    let header = crate::parasolid::StreamHeader {
        body_offset,
        ..crate::parasolid::stream_header(&payload).unwrap().unwrap()
    };
    let spans = node_spans(&[(&payload, &header)])
        .pop()
        .flatten()
        .expect("node-read stream");
    let records = sequence(&payload).unwrap();
    let topology = tables(&records, &payload, body_offset).unwrap();
    for node in records.nodes() {
        let admitted = match node.kind() {
            14 => topology.bridges().contains_key(&node.index()),
            15 => topology.loops().contains_key(&node.index()),
            16 => topology.edge_uses().contains_key(&node.index()),
            17 => topology.coedges().contains_key(&node.index()),
            18 => topology.vertex_uses().contains_key(&node.index()),
            29 => topology.points().contains_key(&node.index()),
            _ => continue,
        };
        let range = node.range();
        let named = spans.iter().any(|span| {
            span.offset == range.start - body_offset
                && span.length == range.len()
                && u16::from(span.tag) == node.kind()
                && span.record == node.index()
        });
        assert_eq!(named, admitted, "node {} {}", node.kind(), node.index());
    }
    assert!(spans
        .windows(2)
        .all(|pair| pair[0].offset <= pair[1].offset));
}

#[test]
fn a_scanned_stream_names_no_node_spans() {
    use crate::test_support::parasolid::{parasolid_with_body, triangle_body};
    let payload = parasolid_with_body("partition body", "SCH_SW_33103_11000", &triangle_body());
    let header = crate::parasolid::stream_header(&payload).unwrap().unwrap();
    assert_eq!(node_spans(&[(&payload, &header)]), vec![None]);
}

fn prefix(node_id: u32) -> Vec<F> {
    vec![F::I(node_id), F::P(1), F::P(1), F::P(1), F::P(1), F::P(1)]
}

fn wrapper(nurbs: u16) -> Vec<F> {
    let mut fields = prefix(1);
    fields.extend([F::C(b'+'), F::P(nurbs), F::P(1)]);
    fields
}

/// `NURBS_CURVE` fields: degree 1, two poles of dimension `dimension`.
fn curve_descriptor(dimension: u16, arrays: [u16; 3]) -> Vec<F> {
    use F::{B, I, L, P, S};
    vec![
        S(1),
        I(2),
        S(dimension),
        I(2),
        B(0),
        L(false),
        L(false),
        L(false),
        B(0),
        P(arrays[0]),
        P(arrays[1]),
        P(arrays[2]),
    ]
}

#[test]
fn a_node_read_stream_names_admitted_analytic_and_spatial_nurbs_nodes() {
    let mut plane = prefix(11);
    plane.extend([
        F::C(b'+'),
        F::V([0.0, 0.0, 0.001]),
        F::V([0.0, 0.0, 1.0]),
        F::V([1.0, 0.0, 0.0]),
    ]);
    let (payload, body_offset) = sequence_of(&[
        (50, 20, plane),
        (134, 3, wrapper(4)),
        (136, 4, curve_descriptor(3, [5, 6, 7])),
        (45, 5, vec![F::Reals(vec![0.0, 0.0, 0.0, 0.001, 0.0, 0.0])]),
        (127, 6, vec![F::Shorts(vec![2, 2])]),
        (128, 7, vec![F::Reals(vec![0.0, 1.0])]),
        (134, 30, wrapper(31)),
        (136, 31, curve_descriptor(2, [32, 33, 34])),
        (45, 32, vec![F::Reals(vec![0.0, 0.0, 1.0, 0.0])]),
        (127, 33, vec![F::Shorts(vec![2, 2])]),
        (128, 34, vec![F::Reals(vec![0.0, 1.0])]),
    ]);
    let header = crate::parasolid::StreamHeader {
        body_offset,
        ..crate::parasolid::stream_header(&payload).unwrap().unwrap()
    };
    let spans = node_spans(&[(&payload, &header)])
        .pop()
        .flatten()
        .expect("node-read stream");
    let named: Vec<(u8, u32)> = spans.iter().map(|span| (span.tag, span.record)).collect();
    assert_eq!(
        named,
        [(50, 20), (134, 3), (136, 4), (45, 5), (127, 6), (128, 7)]
    );
    let records = sequence(&payload).unwrap();
    for (span, node) in spans.iter().zip(records.nodes()) {
        assert_eq!(span.offset, node.range().start - body_offset);
        assert_eq!(span.length, node.range().len());
    }
}

#[test]
fn a_site_whose_deltas_stream_scans_scans_every_deltas_stream() {
    use crate::test_support::parasolid::{parasolid_with_body, world_point};
    use cadmpeg_parasolid::test_support::test_pmarks;
    let complete = test_pmarks::complete(test_pmarks::prefix(1));
    let incomplete = parasolid_with_body(
        "deltas body",
        "SCH_SW_33103_11000",
        &world_point(60, [0.0; 3]),
    );
    let headers: Vec<_> = [&complete, &incomplete]
        .iter()
        .map(|payload| crate::parasolid::stream_header(payload).unwrap().unwrap())
        .collect();
    assert!(node_spans(&[(&complete, &headers[0])])[0].is_some());
    assert_eq!(
        node_spans(&[(&complete, &headers[0]), (&incomplete, &headers[1])]),
        vec![None, None]
    );
}

/// A first-use Deltas analytic node: index, node ID, five status-paired
/// pointers, the sense character, and the raw bytes of its value fields.
fn deltas_analytic(kind: u8, index: u16, values: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0, kind, 255];
    bytes.extend(index.to_be_bytes());
    bytes.extend(7u32.to_be_bytes());
    for _ in 0..5 {
        bytes.extend(1u16.to_be_bytes());
        bytes.push(1);
    }
    bytes.push(b'+');
    bytes.extend(values);
    bytes
}

#[test]
fn a_deltas_analytic_fallback_names_deltas_framed_carriers() {
    use crate::test_support::parasolid::line_carrier;
    use cadmpeg_parasolid::test_support::test_pmarks;
    let line: Vec<u8> = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0f64]
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect();
    let mut plane_values = line_carrier(90, [0.0; 3], [1.0, 0.0, 0.0]);
    plane_values.resize(9 * 8, 0);
    let mut payload = test_pmarks::prefix(1);
    payload.extend(deltas_analytic(30, 20, &line));
    payload.extend(deltas_analytic(50, 20, &plane_values));
    let payload = test_pmarks::complete(payload);
    let header = crate::parasolid::stream_header(&payload).unwrap().unwrap();
    let spans = node_spans(&[(&payload, &header)])
        .pop()
        .flatten()
        .expect("node-read Deltas stream");
    let analytic: Vec<u32> = spans
        .iter()
        .filter(|span| span.tag == 30)
        .map(|span| span.record)
        .collect();
    assert_eq!(analytic, [20]);
}

#[test]
fn a_scanned_stream_names_the_analytic_carriers_of_its_role() {
    use crate::test_support::parasolid::{
        line_carrier, parasolid_with_body, prefixed_line_carrier,
    };
    let mut body = line_carrier(90, [0.0; 3], [1.0, 0.0, 0.0]);
    body.extend(prefixed_line_carrier(91, [0.0; 3], [0.0, 1.0, 0.0]));
    for (description, admitted) in [("partition body", 90), ("deltas body", 91)] {
        let payload = parasolid_with_body(description, "SCH_SW_33103_11000", &body);
        let header = crate::parasolid::stream_header(&payload).unwrap().unwrap();
        let stream_body = &payload[header.body_offset..];
        let analytic: Vec<u32> = super::scanned(stream_body, &header, false)
            .iter()
            .filter(|span| span.tag == 0x1e)
            .map(|span| span.record)
            .collect();
        assert_eq!(analytic, [admitted], "{description}");
    }
}
