// SPDX-License-Identifier: Apache-2.0
//! Topology records the decode leaves out are given up as parts of their
//! stream, and the records that need them are named in that charge.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions, DecodeResult};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::unknown::part::record_unreadable_kind;

use super::{Graph, Node, NodeKind};
use crate::test_support::test_bytes::{put_f64, put_ref, record};
use crate::test_support::test_prt::prt_with_partition;
use crate::test_support::test_streams::{
    primary_stopped_topology_stream, primary_terminated_topology_stream,
    shared_region_shells_partition_stream, topology_partition_stream,
};
use crate::NxCodec;

/// `stream` with its prologue's schema token blanked, so a pinned decode
/// refuses it only for a give-up.
fn schemaless(mut stream: Vec<u8>) -> Vec<u8> {
    let schema = stream
        .windows(4)
        .position(|window| window == b"SCH_")
        .expect("fixture prologue states a schema token");
    stream[schema] = b'X';
    stream
}

fn decode_with(stream: &[u8], pinned: bool) -> Result<DecodeResult, DecodeFailure> {
    let mut options = DecodeOptions::default();
    options.strictness.pinned = pinned;
    NxCodec.decode(&mut Cursor::new(prt_with_partition(stream)), &options)
}

fn decode(stream: &[u8]) -> DecodeResult {
    decode_with(stream, false).expect("a permissive decode reports its give-ups")
}

fn node_at(stream: &[u8], kind: NodeKind, xmt: u32) -> (usize, usize) {
    let graph = Graph::parse(stream);
    let node: &Node = graph.get(kind, xmt).expect("fixture node");
    (node.pos(), node.end() - node.pos())
}

fn is_part_charge(loss: &LossNote, (offset, length): (usize, usize)) -> bool {
    loss.code == record_unreadable_kind()
        && loss.severity == Severity::Error
        && loss
            .message
            .starts_with("part parasolid-record stream=\"nx:parasolid#0:")
        && loss
            .message
            .contains(&format!("\" offset={offset} length={length} record="))
}

/// The charge that gives up the record at `span` as a part of stream 0.
fn part_charge(losses: &[LossNote], span: (usize, usize)) -> &LossNote {
    losses
        .iter()
        .find(|loss| is_part_charge(loss, span))
        .unwrap_or_else(|| panic!("the record at {span:?} is given up as a part: {losses:#?}"))
}

/// The ids a charge names as dropped with its record.
fn dropped(loss: &LossNote) -> Vec<&str> {
    loss.message
        .split_once("dropped=[")
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(ids, _)| ids.split(", ").filter(|id| !id.is_empty()).collect())
        .unwrap_or_default()
}

/// The ids a charge marks.
fn marked(loss: &LossNote) -> Vec<&str> {
    loss.message
        .split_once("marked=[")
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(ids, _)| ids.split(", ").filter(|id| !id.is_empty()).collect())
        .unwrap_or_default()
}

fn has_id<T: AsRef<str>>(ids: impl IntoIterator<Item = T>, id: &str) -> bool {
    ids.into_iter().any(|candidate| candidate.as_ref() == id)
}

#[test]
fn a_vertex_whose_point_does_not_resolve_is_given_up_with_the_records_that_need_it() {
    let mut stream = schemaless(topology_partition_stream());
    let vertex = node_at(&stream, NodeKind::Vertex, 10);
    put_ref(&mut stream, vertex.0 + 16, 99);
    let vertex = node_at(&stream, NodeKind::Vertex, 10);

    let result = decode(&stream);
    let model = &result.ir().geometry;
    assert!(!has_id(
        model.vertices.iter().map(|v| v.id.as_str()),
        "nx:s0:vertex#10"
    ));
    assert!(!has_id(
        model.edges.iter().map(|e| e.id.as_str()),
        "nx:s0:edge#8"
    ));
    assert!(has_id(
        model.faces.iter().map(|f| f.id.as_str()),
        "nx:s0:face#4"
    ));
    let charge = part_charge(&result.report().losses, vertex);
    assert_eq!(
        dropped(charge),
        ["nx:s0:edge#8", "nx:s0:fin#7", "nx:s0:loop#5"],
        "{charge:#?}"
    );
    assert_eq!(marked(charge), ["nx:s0:face#4"], "{charge:#?}");

    match decode_with(&stream, true) {
        Err(DecodeFailure::StrictRejected { rejection }) => {
            assert!(is_part_charge(rejection.loss(), vertex), "{rejection:#?}");
        }
        other => panic!("a pinned decode refuses the give-up: {other:?}"),
    }
}

#[test]
fn an_edge_whose_fin_does_not_resolve_is_given_up_with_its_loop_fin_and_vertex() {
    let mut stream = topology_partition_stream();
    let edge = node_at(&stream, NodeKind::Edge, 8);
    put_ref(&mut stream, edge.0 + 18, 99);
    let edge = node_at(&stream, NodeKind::Edge, 8);

    let result = decode(&stream);
    let charge = part_charge(&result.report().losses, edge);
    assert_eq!(
        dropped(charge),
        ["nx:s0:fin#7", "nx:s0:loop#5", "nx:s0:vertex#10"],
        "{charge:#?}"
    );
    let model = &result.ir().geometry;
    assert!(model.edges.is_empty());
    assert!(model.vertices.is_empty());
    assert!(has_id(
        model.faces.iter().map(|f| f.id.as_str()),
        "nx:s0:face#4"
    ));
}

/// [`topology_partition_stream`] whose shell 3 also holds FACE 14, with
/// LOOP 15, FIN 16 and EDGE 17, on the surface `surface`.
fn two_face_shell_stream(surface: u16) -> Vec<u8> {
    let mut stream = topology_partition_stream();
    let first = node_at(&stream, NodeKind::Face, 4);
    put_ref(&mut stream, first.0 + 18, 14);

    let mut face = record(14, 39);
    put_ref(&mut face, 2, 14);
    put_f64(&mut face, 10, 0.000_2);
    put_ref(&mut face, 18, 1);
    put_ref(&mut face, 20, 4);
    put_ref(&mut face, 22, 15);
    put_ref(&mut face, 24, 3);
    put_ref(&mut face, 26, surface);
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

#[test]
fn a_face_with_no_surface_in_a_kept_shell_is_given_up_with_its_loop() {
    let kept = decode(&two_face_shell_stream(6));
    assert_eq!(kept.ir().geometry.faces.len(), 2);

    let stream = two_face_shell_stream(1);
    let face = node_at(&stream, NodeKind::Face, 14);
    let result = decode(&stream);
    let model = &result.ir().geometry;
    assert!(has_id(
        model.shells.iter().map(|s| s.id.as_str()),
        "nx:s0:shell#3"
    ));
    assert!(has_id(
        model.faces.iter().map(|f| f.id.as_str()),
        "nx:s0:face#4"
    ));
    assert!(!has_id(
        model.faces.iter().map(|f| f.id.as_str()),
        "nx:s0:face#14"
    ));
    let charge = part_charge(&result.report().losses, face);
    assert_eq!(
        dropped(charge),
        ["nx:s0:edge#17", "nx:s0:fin#16", "nx:s0:loop#15"],
        "{charge:#?}"
    );
}

#[test]
fn a_shell_whose_region_another_body_owns_is_given_up_with_its_body_and_faces() {
    let mut stream = shared_region_shells_partition_stream();
    let shell = node_at(&stream, NodeKind::Shell, 13);
    put_ref(&mut stream, shell.0 + 10, 20);
    let shell = node_at(&stream, NodeKind::Shell, 13);

    let result = decode(&stream);
    let model = &result.ir().geometry;
    assert!(has_id(
        model.shells.iter().map(|s| s.id.as_str()),
        "nx:s0:shell#3"
    ));
    assert!(!has_id(
        model.shells.iter().map(|s| s.id.as_str()),
        "nx:s0:shell#13"
    ));
    assert!(!has_id(
        model.bodies.iter().map(|b| b.id.as_str()),
        "nx:s0:body#20"
    ));
    let charge = part_charge(&result.report().losses, shell);
    assert_eq!(
        dropped(charge),
        [
            "nx:s0:body#20",
            "nx:s0:edge#17",
            "nx:s0:face#14",
            "nx:s0:fin#16",
            "nx:s0:loop#15"
        ],
        "{charge:#?}"
    );
}

#[test]
fn a_withheld_body_with_no_record_is_named_by_no_charge() {
    // SHELL 3 names BODY 20, which no record carries, and the shell is
    // withheld because its one face has no surface.
    let mut stream = topology_partition_stream();
    let shell = node_at(&stream, NodeKind::Shell, 3);
    put_ref(&mut stream, shell.0 + 10, 20);
    let face = node_at(&stream, NodeKind::Face, 4);
    put_ref(&mut stream, face.0 + 26, 1);

    let result = decode(&stream);
    let losses = &result.report().losses;
    assert!(losses.iter().any(|loss| {
        loss.code == crate::loss::NxLossCode::TopologyShellWithheld.kind()
            && loss.message.contains("BODY 20")
    }));
    assert!(
        losses
            .iter()
            .filter(|loss| loss.code == record_unreadable_kind())
            .all(|loss| !has_id(
                loss.message
                    .split(|c: char| c.is_whitespace() || matches!(c, '[' | ']' | ',')),
                "nx:s0:body#20"
            )),
        "{losses:#?}"
    );
}

#[test]
fn the_loops_of_a_face_whose_loop_chain_does_not_resolve_are_given_up_with_their_fins() {
    // LOOP 5 names no FIN, so FACE 4's loop chain does not resolve and the
    // face is stated partial with none of its loops.
    let mut stream = schemaless(topology_partition_stream());
    let loop_ = node_at(&stream, NodeKind::Loop, 5);
    put_ref(&mut stream, loop_.0 + 10, 1);
    let loop_ = node_at(&stream, NodeKind::Loop, 5);

    let result = decode(&stream);
    let model = &result.ir().geometry;
    assert!(has_id(
        model.faces.iter().map(|f| f.id.as_str()),
        "nx:s0:face#4"
    ));
    assert!(model.loops.is_empty());
    assert!(model.edges.is_empty());
    let charge = part_charge(&result.report().losses, loop_);
    assert_eq!(
        dropped(charge),
        ["nx:s0:edge#8", "nx:s0:fin#7", "nx:s0:vertex#10"],
        "{charge:#?}"
    );
    assert_eq!(marked(charge), ["nx:s0:face#4"], "{charge:#?}");

    match decode_with(&stream, true) {
        Err(DecodeFailure::StrictRejected { rejection }) => {
            assert!(is_part_charge(rejection.loss(), loop_), "{rejection:#?}");
        }
        other => panic!("a pinned decode refuses the give-up: {other:?}"),
    }
}

#[test]
fn a_vertex_whose_point_the_primary_framing_refuses_is_dropped_with_the_point() {
    let framed = primary_terminated_topology_stream([0.01, 0.02, 0.03]);
    let primary = cadmpeg_parasolid::records::PrimaryRecords::read(&framed)
        .expect("printable schema")
        .expect("base-13006 transmit header");
    assert_eq!(primary.stop_offset(), None);
    let graph = Graph::parse_source(&framed).expect("terminated primary sequence");
    assert!(graph.primary_node_refusals().is_empty());
    assert!(graph.get(NodeKind::Point, 11).is_some());

    let stream = primary_terminated_topology_stream([1.0e308, 0.0, 0.0]);
    let graph = Graph::parse_source(&stream).expect("terminated primary sequence");
    let [(NodeKind::Point, 11, point)] = graph.primary_node_refusals() else {
        panic!(
            "the framing refuses POINT 11 only: {:?}",
            graph.primary_node_refusals()
        );
    };
    let point = (point.start, point.len());
    let vertex = graph.get(NodeKind::Vertex, 10).expect("framed VERTEX");
    let vertex = (vertex.pos(), vertex.end() - vertex.pos());

    let result = decode(&stream);
    let losses = &result.report().losses;
    let charge = part_charge(losses, point);
    assert_eq!(
        dropped(charge),
        [
            "nx:s0:edge#8",
            "nx:s0:fin#7",
            "nx:s0:loop#5",
            "nx:s0:vertex#10"
        ],
        "{charge:#?}"
    );
    assert_eq!(marked(charge), ["nx:s0:face#4"], "{charge:#?}");
    assert!(
        !losses.iter().any(|loss| is_part_charge(loss, vertex)),
        "{losses:#?}"
    );
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code == record_unreadable_kind())
            .filter(|loss| loss.message.contains("nx:s0:vertex#10"))
            .count(),
        1,
        "{losses:#?}"
    );
    assert!(has_id(
        result.ir().geometry.faces.iter().map(|f| f.id.as_str()),
        "nx:s0:face#4"
    ));
}

/// The `(offset, length)` of each part of stream 0 of `kind` that `losses`
/// gives up.
fn part_spans(losses: &[LossNote], kind: &str) -> Vec<(usize, usize)> {
    losses
        .iter()
        .filter(|loss| loss.code == record_unreadable_kind())
        .filter(|loss| {
            loss.message
                .starts_with(&format!("part {kind} stream=\"nx:parasolid#0:"))
        })
        .filter_map(|loss| {
            let field = |name: &str| -> Option<usize> {
                let (_, rest) = loss.message.split_once(&format!(" {name}="))?;
                rest.split(|c: char| !c.is_ascii_digit())
                    .next()?
                    .parse()
                    .ok()
            };
            Some((field("offset")?, field("length")?))
        })
        .collect()
}

#[test]
fn the_unframed_parts_past_a_primary_stop_hold_no_record_the_decode_reads() {
    let stream = primary_stopped_topology_stream(1.0e-6);
    let graph = Graph::parse_source(&stream).expect("stopped primary sequence");
    let stop = graph
        .primary_unread_from()
        .expect("the primary framing stops");
    let edge = node_at(&stream, NodeKind::Edge, 8);
    assert!(graph.get(NodeKind::Edge, 8).is_none());
    assert!(graph.of_kind(NodeKind::Face).any(|face| face.pos() > stop));

    let result = decode(&stream);
    assert!(has_id(
        result.ir().geometry.faces.iter().map(|f| f.id.as_str()),
        "nx:s0:face#4"
    ));
    let losses = &result.report().losses;
    let unframed = part_spans(losses, "parasolid-unframed");
    let covers = |at: usize| {
        unframed
            .iter()
            .any(|&(offset, length)| (offset..offset + length).contains(&at))
    };
    assert!(covers(edge.0), "{unframed:?}");
    for kind in [
        NodeKind::Body,
        NodeKind::Shell,
        NodeKind::Face,
        NodeKind::Loop,
        NodeKind::Fin,
        NodeKind::Plane,
        NodeKind::Line,
        NodeKind::Vertex,
        NodeKind::Region,
        NodeKind::Point,
    ] {
        for node in graph.of_kind(kind) {
            assert!(
                !covers(node.pos()),
                "{kind:?} at {}: {unframed:?}",
                node.pos()
            );
        }
    }
    for (offset, _) in part_spans(losses, "parasolid-record") {
        assert!(!covers(offset), "{offset}: {unframed:?}");
    }
}

#[test]
fn the_unframed_parts_past_a_primary_stop_leave_out_a_scanned_support_uv_record() {
    let mut stream = primary_stopped_topology_stream(1.0e-6);
    let at = stream.len();
    let mut uv = record(204, 9 + 2 * 8);
    uv[2..6].copy_from_slice(&2_u32.to_be_bytes());
    put_ref(&mut uv, 6, 20);
    uv[8] = 2;
    stream.extend(&uv);
    assert!(crate::intersection::support_uv_records(&stream)
        .iter()
        .any(|record| record.pos == at));

    let result = decode(&stream);
    let unframed = part_spans(&result.report().losses, "parasolid-unframed");
    assert!(
        unframed
            .iter()
            .all(|&(offset, length)| offset + length <= at),
        "{unframed:?}"
    );
}
