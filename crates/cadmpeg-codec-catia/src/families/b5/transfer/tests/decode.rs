// SPDX-License-Identifier: Apache-2.0
//! B5 dump tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::CatiaLossCode;
use crate::test_support::*;
use crate::variant::Variant;
use crate::CatiaCodec;

#[test]
fn decode_float_packed_stream_transfers_reference_closed_b5_topology() {
    let mut stream = b5_closed_triangle_stream();
    append_b5_record(
        &mut stream,
        0x5e,
        900,
        &[
            0x85, 0x81, 0x18, 0x85, 0x03, 0x18, 0x85, 0x03, 0x81, 0x81, 0x2a,
        ],
    );
    append_b5_record(&mut stream, 0x5d, 901, &[0x81, 0x81, 0x04]);
    crate::families::b5::graph::parse(&stream).expect("generated B5 topology");
    let file = object_main_catpart(&stream);
    assert_eq!(
        crate::container::scan_bytes(file.clone()).variant,
        Variant::FloatPackedInnerNoFbb
    );

    let mut cur = Cursor::new(file);
    let result = CatiaCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.curves.len(), 3);
    assert!(result.ir().model.surfaces.iter().all(|surface| {
        surface.source_object.as_ref().is_some_and(|source| {
            source.format == "catia" && source.object_id.starts_with("cgm-surface:")
        })
    }));
    assert!(result.ir().model.curves.iter().all(|curve| {
        curve.source_object.as_ref().is_some_and(|source| {
            source.format == "catia" && source.object_id.starts_with("cgm-edge:")
        })
    }));
    assert_eq!(result.ir().model.procedural_curves.len(), 3);
    assert!(result.ir().model.procedural_curves.iter().all(|curve| {
        matches!(
            curve.definition(),
            cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
                ref context,
                ..
            } if context.sides[0].surface.is_some()
                && context.sides[0].pcurve.is_some()
                && context.sides[1].surface.is_none()
        )
    }));
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.pcurves.len(), 3);
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::RESOLVED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_03_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::RESOLVED_OBJECT_STREAM_FACE_TERMINAL_CONTROL_05_COUNT),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::RESOLVED_OBJECT_STREAM_UNCOUNTED_FACE_COUNT),
        0
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::TYPED_OBJECT_STREAM_EDGE_TERMINAL_CONTROL_2A_COUNT),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::TYPED_OBJECT_STREAM_VERTEX_INCIDENCE_TERMINAL_CONTROL_04_COUNT
        ),
        1
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::RESOLVED_OBJECT_STREAM_LOOP_FRAMING_CONTROLS_05_05_COUNT
        ),
        1
    );
    assert_eq!(
        result
            .report()
            .coverage_count(crate::coverage::RESOLVED_OBJECT_STREAM_EXTENDED_LOOP_METADATA_COUNT),
        0
    );
    assert_eq!(
        result.report().coverage_count(
            crate::coverage::RESOLVED_OBJECT_STREAM_CLASS_21_PCURVE_SUFFIX_SCALAR_COUNT
        ),
        3
    );
    assert!(result
        .ir()
        .model
        .pcurves
        .iter()
        .all(|pcurve| pcurve.parameter_range == Some([0.0, 1.0])));
    assert!(result.report().losses.iter().all(|loss| {
        !matches!(
            loss.code.category(),
            cadmpeg_ir::report::LossCategory::Geometry | cadmpeg_ir::report::LossCategory::Topology
        ) || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_float_packed_stream_transfers_a_complete_native_vertex_chain() {
    let stream = b5_closed_triangle_stream_with_native_vertex_chain();
    let graph = crate::families::b5::graph::parse(&stream).expect("generated B5 topology");
    assert!(graph.complete);
    assert_eq!(graph.vertex_incidence_links.len(), 3);
    assert_eq!(graph.parameter_incidences.len(), 3);
    assert_eq!(graph.edges.len(), 3);
    assert_eq!(graph.edge_parameter_incidences.len(), 3);
    assert_eq!(graph.logical_vertex_refs, [600, 601, 602]);
    assert_eq!(
        graph.logical_vertex_points,
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    );

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode complete native vertex chain");
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert!(result.report().losses.iter().all(|loss| {
        !matches!(
            loss.code.category(),
            cadmpeg_ir::report::LossCategory::Geometry | cadmpeg_ir::report::LossCategory::Topology
        ) || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

/// A `b5 03` object id reaches the neutral model as an unpadded decimal key, so
/// an edge triple such as `9`, `10`, `11` is emitted in ascending native order
/// and sorts the other way. The route must still transfer the topology: a
/// cross-reference is an id string, so arena order carries no reference
/// semantics and the pipeline restores it.

#[test]
fn decode_float_packed_stream_transfers_topology_under_decimal_object_ids() {
    let mut stream = b5_closed_triangle_stream_over_edges([9, 10, 11]);
    append_b5_record(
        &mut stream,
        0x5e,
        900,
        &[
            0x85, 0x81, 0x18, 0x85, 0x03, 0x18, 0x85, 0x03, 0x81, 0x81, 0x2a,
        ],
    );
    append_b5_record(&mut stream, 0x5d, 901, &[0x81, 0x81, 0x04]);
    crate::families::b5::graph::parse(&stream).expect("generated B5 topology");

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode object-stream topology");

    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.pcurves.len(), 3);
    assert_eq!(
        result
            .ir()
            .model
            .edges
            .iter()
            .map(|edge| edge.id.0.as_str())
            .collect::<Vec<_>>(),
        ["catia:b5:edge#10", "catia:b5:edge#11", "catia:b5:edge#9"]
    );
    assert!(result.report().losses.iter().all(|loss| {
        !matches!(
            loss.code.category(),
            cadmpeg_ir::report::LossCategory::Geometry | cadmpeg_ir::report::LossCategory::Topology
        ) || loss.severity != cadmpeg_ir::report::Severity::Blocking
    }));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new());
    assert!(validation.is_ok(), "findings: {:?}", validation.findings);
}

#[test]
fn decode_does_not_transfer_a_loop_with_multiple_face_owners() {
    let mut stream = b5_closed_triangle_stream();
    let mut face_payload = vec![0x82];
    face_payload.extend_from_slice(&b5_object_ref(100));
    face_payload.extend_from_slice(&b5_object_ref(400));
    face_payload.push(0x03);
    append_b5_record(&mut stream, 0x5f, 902, &face_payload);

    let result = CatiaCodec
        .decode(
            &mut Cursor::new(object_main_catpart(&stream)),
            &DecodeOptions::default(),
        )
        .expect("decode duplicate loop-owner stream");

    assert!(result.ir().model.bodies.is_empty());
    assert!(result.ir().model.faces.is_empty());
    assert!(result.report().losses.iter().any(|loss| {
        loss.code == CatiaLossCode::TopologyB5GraphUnclosed.kind()
            && loss.severity == cadmpeg_ir::report::Severity::Blocking
    }));
}
