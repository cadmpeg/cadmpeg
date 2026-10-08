// SPDX-License-Identifier: Apache-2.0
//! topology walk tests.

use super::{
    collect_object_stream_frames, edge_support_pcurve_references_from_frames,
    edge_vertex_references, face_loop_owner_counts, face_surface_references_from_frames,
    implicit_pcurve_bindings, parameter_incidence, parse, parse_edge, parse_vertex_incidence_link,
    propagate_vertex_points, records, surface_node, typed_parameter_incidences_from_records,
    typed_vertex_incidence_rosters_from_records, B5Edge, B5EdgeTerminalControl, B5IncidenceLane,
    B5RecordBuf, B5Surface, B5VertexIncidenceControl, B5VertexIncidenceLink, BTreeMap, HashMap,
};
use crate::families::b5::graph::{parse_loop_record, B5Record};

/// Binds implicit pcurves over owned records, deriving the loops, edges and
/// parameter incidences the binder reads from them.
fn bind_implicit_pcurves(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &[B5RecordBuf],
    surfaces: &BTreeMap<u32, B5Surface>,
) -> Result<BTreeMap<u32, u32>, cadmpeg_core::CodecError> {
    let views: Vec<B5Record<'_>> = records.iter().map(B5RecordBuf::record).collect();
    let by_id: HashMap<u32, &B5Record<'_>> = views
        .iter()
        .map(|record| (record.object_id, record))
        .collect();
    let mut loops = Vec::new();
    let mut edges = BTreeMap::new();
    let mut incidences = BTreeMap::new();
    for record in &views {
        match record.class {
            0x62 => loops.extend(parse_loop_record(ctx, record)?),
            0x5e => {
                if let Some(edge) = parse_edge(record) {
                    edges.insert(record.object_id, edge);
                }
            }
            0x06 => {
                if let Some(incidence) = parameter_incidence(ctx, record)? {
                    incidences.insert(record.object_id, incidence);
                }
            }
            _ => {}
        }
    }
    implicit_pcurve_bindings(
        ctx,
        &loops,
        &by_id,
        &edges,
        &incidences,
        (&BTreeMap::new(), &BTreeMap::new()),
        surfaces,
    )
}

#[test]
fn record_walk_includes_wide_header_loop_nodes() {
    let mut bytes = vec![0xa8, 0x03, 0x62];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&[0x83, 0x81, 0x82]);
    bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
    bytes.extend_from_slice(&8u32.to_le_bytes());
    let expected = [
        B5RecordBuf {
            offset: 0,
            family: 0xa8,
            class: 0x62,
            object_id: 7,
            payload: vec![0x83, 0x81, 0x82],
        },
        B5RecordBuf {
            offset: 14,
            family: 0xb5,
            class: 0x5e,
            object_id: 8,
            payload: Vec::new(),
        },
    ];
    assert_eq!(
        records(&bytes),
        expected.iter().map(B5RecordBuf::record).collect::<Vec<_>>()
    );
}

#[test]
fn record_walk_retains_opaque_a8_surface_nodes() {
    let mut bytes = vec![0xa8, 0x03, 0x34];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&[1, 2, 3]);
    bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
    bytes.extend_from_slice(&8u32.to_le_bytes());
    let records = records(&bytes);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].family, 0xa8);
    assert_eq!(records[0].class, 0x34);
    assert_eq!(records[0].object_id, 7);
    assert_eq!(records[0].payload, [1, 2, 3]);
    let limited = crate::test_support::with_retained_limit(2, |ctx| {
        surface_node(ctx, &records[0], &BTreeMap::new())
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_surface_node_payload")
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| surface_node(
            ctx,
            &records[0],
            &BTreeMap::new()
        ))
        .expect("service budget"),
        Some(B5Surface::Unknown {
            family: 0xa8,
            class: 0x34,
            payload: vec![1, 2, 3],
        })
    );
}

#[test]
fn record_walk_descends_into_length_bounded_a8_wrappers() {
    let mut payload = vec![0xb5, 0x03, 0x27, 0x00];
    payload.extend_from_slice(&1u32.to_le_bytes());
    payload.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
    payload.extend_from_slice(&2u32.to_le_bytes());

    let mut bytes = vec![0xa8, 0x03, 0x34];
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&payload);
    bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
    bytes.extend_from_slice(&3u32.to_le_bytes());

    assert_eq!(
        records(&bytes)
            .iter()
            .map(|record| (record.offset, record.object_id, record.class))
            .collect::<Vec<_>>(),
        vec![(11, 1, 0x27), (19, 2, 0x5e), (0, 7, 0x34), (27, 3, 0x5e)]
    );
}

#[test]
fn record_walk_crosses_alternate_flag_bridge_records() {
    let mut bytes = vec![0xb5, 0x03, 0x27, 0x00];
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0xb5, 0x13, 0x5b, 0x00]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0xb5, 0x03, 0x5e, 0x00]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    assert_eq!(
        records(&bytes)
            .iter()
            .map(|record| (record.object_id, record.class))
            .collect::<Vec<_>>(),
        vec![(1, 0x27), (3, 0x5e)]
    );
}

#[test]
fn record_walk_admits_unique_isolated_geometry_by_topology_reference() {
    fn append(bytes: &mut Vec<u8>, class: u8, object_id: u32, payload: &[u8]) {
        bytes.extend_from_slice(&[
            0xb5,
            0x03,
            class,
            u8::try_from(payload.len()).expect("test payload fits the B5 length lane"),
        ]);
        bytes.extend_from_slice(&object_id.to_le_bytes());
        bytes.extend_from_slice(payload);
    }

    let mut bytes = Vec::new();
    append(&mut bytes, 0x27, 1, &[]);
    bytes.push(0xff);
    append(&mut bytes, 0x19, 2, &[]);
    bytes.push(0xff);
    append(&mut bytes, 0x62, 4, &[0x83, 0x82, 0x83, 0x81]);
    append(&mut bytes, 0x5e, 3, &[]);
    append(&mut bytes, 0x5f, 5, &[0x82, 0x81, 0x84]);

    let parsed = records(&bytes);
    assert_eq!(
        parsed
            .iter()
            .map(|record| (record.object_id, record.class))
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([(1, 0x27), (2, 0x19), (3, 0x5e), (4, 0x62), (5, 0x5f),])
    );

    bytes.push(0xff);
    append(&mut bytes, 0x19, 2, &[0x01]);
    assert!(!records(&bytes).iter().any(|record| record.object_id == 2));
}

#[test]
fn record_walk_closes_native_vertex_incidence_dependencies() {
    fn append(bytes: &mut Vec<u8>, class: u8, object_id: u32, payload: &[u8]) {
        bytes.extend_from_slice(&[
            0xb5,
            0x03,
            class,
            u8::try_from(payload.len()).expect("fixture value fits u8"),
        ]);
        bytes.extend_from_slice(&object_id.to_le_bytes());
        bytes.extend_from_slice(payload);
    }

    let mut bytes = Vec::new();
    append(&mut bytes, 0x18, 2, &[0x81, 0x81]);
    bytes.push(0xff);
    append(&mut bytes, 0x5d, 6, &[0x81, 0x87, 0x00]);
    bytes.push(0xff);
    append(&mut bytes, 0x05, 7, &[0x81, 0x88]);
    bytes.push(0xff);
    let mut parameter = vec![0x81, 0x82, 0x81];
    parameter.extend_from_slice(&0.5f64.to_le_bytes());
    parameter.push(0x05);
    append(&mut bytes, 0x06, 8, &parameter);
    bytes.push(0xff);
    append(
        &mut bytes,
        0x5e,
        10,
        &[0x85, 0x82, 0x86, 0x86, 0x88, 0x88, 0x21],
    );
    append(&mut bytes, 0x5f, 11, &[]);

    assert_eq!(
        records(&bytes)
            .iter()
            .map(|record| (record.object_id, record.class))
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            (2, 0x18),
            (6, 0x5d),
            (7, 0x05),
            (8, 0x06),
            (10, 0x5e),
            (11, 0x5f),
        ])
    );
}

#[test]
fn native_vertex_graph_rejects_inconsistent_ordered_loci() {
    let points = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 5e-3, 0.0],
        [0.0, 5e-3, 0.0],
    ];
    let constraints = [([10, 11], [0, 1]), ([11, 12], [2, 3]), ([12, 10], [3, 0])];
    let adjacency = HashMap::from([(10, vec![0, 2]), (11, vec![0, 1]), (12, vec![1, 2])]);
    let mapping = propagate_vertex_points(
        &constraints,
        &adjacency,
        &points.map(crate::test_support::test_b5::point),
    );
    assert!(mapping.is_empty());
}

#[test]
fn edge_record_retains_references_and_each_admitted_terminal_control() {
    let record = B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x5e,
        object_id: 17,
        payload: vec![0x85, 0x92, 0x8f, 0x95, 0x93, 0x94, 0x21],
    };
    assert_eq!(
        parse_edge(&record.record()),
        Some(B5Edge {
            object_id: 17,
            support: 18,
            vertices: [15, 21],
            parameter_incidences: [19, 20],
            terminal_control: B5EdgeTerminalControl::Control21,
        })
    );

    let mut standard = record;
    for terminal_control in [0x01, 0x02, 0x21, 0x22, 0x25, 0x26, 0x29, 0x2a] {
        *standard.payload.last_mut().expect("tail") = terminal_control;
        assert_eq!(
            parse_edge(&standard.record()).map(|edge| edge.terminal_control.as_byte()),
            Some(terminal_control)
        );
    }
    standard.payload.pop();
    assert!(parse_edge(&standard.record()).is_none());
    standard.payload.extend_from_slice(&[0x21, 0x00]);
    assert!(parse_edge(&standard.record()).is_none());
    standard.payload.truncate(6);
    standard.payload.push(0x03);
    assert!(parse_edge(&standard.record()).is_none());
    *standard.payload.last_mut().expect("tail") = 0x01;

    let mut bytes = vec![0xb5, 0x03, 0x5e, 7];
    bytes.extend_from_slice(&standard.object_id.to_le_bytes());
    bytes.extend_from_slice(&standard.payload);
    assert_eq!(
        crate::test_support::with_service_context(|ctx| { edge_vertex_references(ctx, &bytes) })
            .expect("service budget"),
        BTreeMap::from([(17, [15, 21])])
    );
}

#[test]
fn referenced_edge_vertex_references_excludes_unreferenced_allocations() {
    let mut graph = crate::test_support::with_service_context(|ctx| {
        parse(
            ctx,
            &crate::test_support::test_b5::b5_closed_triangle_stream(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("service resource budget")
    .expect("B5 graph");
    assert!(graph.complete);
    graph.edges.insert(
        301,
        B5Edge {
            object_id: 301,
            support: 600,
            vertices: [10, 11],
            parameter_incidences: [20, 21],
            terminal_control: B5EdgeTerminalControl::Control01,
        },
    );
    graph.edges.insert(
        900,
        B5Edge {
            object_id: 900,
            support: 601,
            vertices: [12, 13],
            parameter_incidences: [22, 23],
            terminal_control: B5EdgeTerminalControl::Control01,
        },
    );

    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            graph.referenced_edge_vertex_references(ctx)
        })
        .expect("service budget"),
        Some(BTreeMap::from([(301, [10, 11])]))
    );

    graph.complete = false;
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            graph.referenced_edge_vertex_references(ctx)
        })
        .expect("service budget"),
        None
    );
}

#[test]
fn b5_topology_reference_helpers_refuse_caller_limits() {
    let bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut graph = crate::test_support::with_service_context(|ctx| {
        parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service budget")
    .expect("closed graph");
    graph.edges.insert(
        301,
        B5Edge {
            object_id: 301,
            support: 600,
            vertices: [10, 11],
            parameter_incidences: [20, 21],
            terminal_control: B5EdgeTerminalControl::Control01,
        },
    );
    let mut reference_bytes = Vec::new();
    crate::test_support::test_b5::append_b5_record(
        &mut reference_bytes,
        0x5e,
        17,
        &[0x85, 0x92, 0x8f, 0x95, 0x93, 0x94, 0x21],
    );
    crate::test_support::test_b5::append_b5_record(
        &mut reference_bytes,
        0x23,
        18,
        &[0x82, 0x89, 0x8a],
    );
    crate::test_support::test_b5::append_b5_record(&mut reference_bytes, 0x5f, 19, &[0x81, 0x89]);
    let frames = crate::test_support::with_service_context(|ctx| {
        collect_object_stream_frames(ctx, &reference_bytes)
    })
    .expect("service budget");
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        graph.referenced_edge_vertex_references(ctx)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_referenced_edge_vertices")
    );
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        face_loop_owner_counts(ctx, &graph.faces)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_face_loop_owners")
    );
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        edge_vertex_references(ctx, &reference_bytes)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_edge_vertex_references")
    );
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        face_surface_references_from_frames(ctx, &reference_bytes, &frames)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_face_surface_references")
    );
    let edge_ids = std::collections::HashSet::from([17]);
    // The candidate frames are parsed in place, so the wrapper index is the
    // first collection the caller's limit refuses.
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        edge_support_pcurve_references_from_frames(ctx, &reference_bytes, &edge_ids, &frames)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(error))
        if error.operation == "catia_b5_edge_support_wrappers")
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            edge_vertex_references(ctx, &reference_bytes)
        })
        .expect("service budget"),
        BTreeMap::from([(17, [15, 21])])
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            face_surface_references_from_frames(ctx, &reference_bytes, &frames)
        })
        .expect("service budget"),
        [(19, 9)]
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            edge_support_pcurve_references_from_frames(ctx, &reference_bytes, &edge_ids, &frames)
        })
        .expect("service budget"),
        BTreeMap::from([(17, [9, 10])])
    );
}

#[test]
fn duplicate_face_loop_ownership_does_not_close_the_graph() {
    let mut bytes = crate::test_support::test_b5::b5_closed_triangle_stream();
    let mut face_payload = vec![0x82];
    face_payload.extend_from_slice(&crate::test_support::test_b5::b5_object_ref(100));
    face_payload.extend_from_slice(&crate::test_support::test_b5::b5_object_ref(400));
    face_payload.push(0x03);
    crate::test_support::test_b5::append_b5_record(&mut bytes, 0x5f, 902, &face_payload);

    let graph = crate::test_support::with_service_context(|ctx| {
        parse(ctx, &bytes, &mut crate::nurbs::LaneRefusals::new())
    })
    .expect("service resource budget")
    .expect("structurally parseable B5 graph");

    assert_eq!(graph.faces.len(), 2);
    assert_eq!(graph.loops.len(), 1);
    assert!(!graph.complete);
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            face_loop_owner_counts(ctx, &graph.faces)
        })
        .expect("service budget")
        .get(&400),
        Some(&2)
    );
}

#[test]
fn vertex_incidence_link_accepts_both_exact_terminal_controls() {
    let record = |terminal_control| B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x5d,
        object_id: 17,
        payload: vec![0x81, 0x92, terminal_control],
    };
    for terminal_control in [0x00, 0x04] {
        assert_eq!(
            B5VertexIncidenceControl::from_byte(terminal_control)
                .expect("declared terminal control")
                .as_byte(),
            terminal_control
        );
        assert_eq!(
            parse_vertex_incidence_link(&record(terminal_control).record()),
            Some(B5VertexIncidenceLink {
                object_id: 17,
                incidence: 18,
                terminal_control: B5VertexIncidenceControl::from_byte(terminal_control)
                    .expect("declared terminal control"),
            })
        );
    }
    assert_eq!(parse_vertex_incidence_link(&record(0x01).record()), None);

    let mut missing = record(0x00);
    missing.payload.pop();
    assert_eq!(parse_vertex_incidence_link(&missing.record()), None);

    let mut residual = record(0x04);
    residual.payload.push(0);
    assert_eq!(parse_vertex_incidence_link(&residual.record()), None);
}

#[test]
fn loop_and_endpoint_incidences_bind_an_unframed_pcurve_occurrence() {
    let incidence_payload = |parameter: f64, control| {
        let mut payload = vec![0x81, 0x89, 0x81];
        payload.extend_from_slice(&parameter.to_le_bytes());
        payload.push(control);
        payload
    };
    let records = vec![
        B5RecordBuf {
            offset: 0,
            family: 0xb5,
            class: 0x62,
            object_id: 1,
            payload: vec![
                0x83, 0x89, 0x8a, 0x8b, 0x81, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00,
                0x01,
            ],
        },
        B5RecordBuf {
            offset: 1,
            family: 0xb5,
            class: 0x5e,
            object_id: 10,
            payload: vec![0x85, 0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x21],
        },
        B5RecordBuf {
            offset: 2,
            family: 0xb5,
            class: 0x06,
            object_id: 15,
            payload: incidence_payload(0.0, 0x15),
        },
        B5RecordBuf {
            offset: 3,
            family: 0xb5,
            class: 0x06,
            object_id: 16,
            payload: incidence_payload(1.0, 0x05),
        },
    ];
    let surfaces = BTreeMap::from([(
        11,
        B5Surface::Unknown {
            family: 0xb5,
            class: 0x34,
            payload: Vec::new(),
        },
    )]);

    let service = crate::test_support::with_service_context(|ctx| {
        bind_implicit_pcurves(ctx, &records, &surfaces)
    })
    .expect("service budget");
    assert_eq!(service, BTreeMap::from([(9, 11)]));
}

#[test]
fn parameter_incidence_retains_aligned_compact_controls() {
    let mut payload = vec![0x82, 0x89, 0x8a, 0x82];
    payload.extend_from_slice(&1.25f64.to_le_bytes());
    payload.push(0x15);
    payload.extend_from_slice(&2.5f64.to_le_bytes());
    payload.push(0x2d);
    let incidence = crate::test_support::with_service_context(|ctx| {
        parameter_incidence(
            ctx,
            &B5RecordBuf {
                offset: 0,
                family: 0xb5,
                class: 0x06,
                object_id: 17,
                payload,
            }
            .record(),
        )
    })
    .expect("service budget")
    .expect("parameter incidence");

    assert_eq!(incidence.object_id, 17);
    assert_eq!(
        incidence.lanes,
        [
            B5IncidenceLane {
                curve: 9,
                parameter: crate::test_support::test_b5::finite(1.25),
                control: 5,
            },
            B5IncidenceLane {
                curve: 10,
                parameter: crate::test_support::test_b5::finite(2.5),
                control: 11,
            },
        ]
    );
}

#[test]
fn parameter_and_roster_incidence_refuse_each_collection_boundary() {
    let roster = B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x05,
        object_id: 18,
        payload: vec![0x82, 0x89, 0x8a],
    };
    let mut payload = vec![0x82, 0x89, 0x8a, 0x82];
    payload.extend_from_slice(&1.25f64.to_le_bytes());
    payload.push(0x15);
    payload.extend_from_slice(&2.5f64.to_le_bytes());
    payload.push(0x2d);
    let parameter = B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x06,
        object_id: 17,
        payload,
    };
    for (limit, operation) in [
        (1, "catia_b5_parameter_incidence_references"),
        (2, "catia_b5_parameter_incidence_lanes"),
        (4, "catia_b5_typed_parameter_incidences"),
    ] {
        let result = crate::test_support::with_collection_limit(limit, |ctx| {
            typed_parameter_incidences_from_records(ctx, &[parameter.record()])
        });
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == operation)
        );
    }
    for (limit, operation) in [
        (1, "catia_wire_counted_references"),
        (2, "catia_b5_typed_vertex_incidence_rosters"),
    ] {
        let result = crate::test_support::with_collection_limit(limit, |ctx| {
            typed_vertex_incidence_rosters_from_records(ctx, &[roster.record()])
        });
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(error))
            if error.operation == operation)
        );
    }
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            typed_parameter_incidences_from_records(ctx, &[parameter.record()])
        })
        .expect("service budget")[&17]
            .lanes
            .len(),
        2
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            typed_vertex_incidence_rosters_from_records(ctx, &[roster.record()])
        })
        .expect("service budget")[&18],
        [9, 10]
    );
}

#[test]
fn loop_and_edge_curve_wrapper_bind_an_unframed_pcurve_occurrence() {
    let records = vec![
        B5RecordBuf {
            offset: 0,
            family: 0xb5,
            class: 0x62,
            object_id: 1,
            payload: vec![
                0x83, 0x89, 0x8a, 0x8b, 0x81, 0x05, 0x05, 0x03, 0x01, 0x00, 0xff, 0xff, 0x01, 0x00,
                0x01,
            ],
        },
        B5RecordBuf {
            offset: 1,
            family: 0xb5,
            class: 0x5e,
            object_id: 10,
            payload: vec![0x85, 0x8c, 0x8d, 0x8e, 0x8f, 0x90, 0x22],
        },
        B5RecordBuf {
            offset: 2,
            family: 0xb5,
            class: 0x25,
            object_id: 12,
            payload: vec![0x82, 0x89, 0x91, 0x81],
        },
    ];
    let surfaces = BTreeMap::from([(
        11,
        B5Surface::Unknown {
            family: 0xb5,
            class: 0x34,
            payload: Vec::new(),
        },
    )]);

    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            bind_implicit_pcurves(ctx, &records, &surfaces).expect("service budget")
        }),
        BTreeMap::from([(9, 11)])
    );

    let refused =
        crate::test_support::with_work_refusal("catia_b5_curve_wrapper_pcurve_search", |ctx| {
            let result = bind_implicit_pcurves(ctx, &records, &surfaces);
            if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(
                    limit.dimension,
                    cadmpeg_core::decode::ResourceDimension::WorkUnits
                );
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        });
    assert!(matches!(
        refused,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                && limit.operation == "catia_b5_curve_wrapper_pcurve_search"
    ));
}
