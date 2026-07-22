// SPDX-License-Identifier: Apache-2.0
//! Record-decoder tests for the `consolidated` family over synthetic byte fixtures.

#![allow(clippy::unwrap_used)]

use crate::tests::{
    a5_circle_bound_edge_stream, a5_cone_bound_edge_stream, a5_cylinder_bound_edge_stream,
    a5_edge_block_stream, a5_native_edge_run_stream, a5_nurbs_bound_edge_stream, a5_pcurve_stream,
    a6_pcurve_stream, append_b5_record, b2_edge_block_stream, b2_edge_parameter_stream_for,
    b2_topology_edge_run_stream, b3_cylinder_stream, le_f32,
};

#[test]
fn object_stream_vertices_exclude_framed_payload_markers() {
    let mut bytes = vec![0xb2, 0x03, 0x06, 0x10, 0x05];
    bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
    for value in [90.0f32, 91.0, 92.0] {
        bytes.extend_from_slice(&le_f32(value));
    }
    bytes.push(0);
    bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
    for value in [1.0f32, 2.0, 3.0] {
        bytes.extend_from_slice(&le_f32(value));
    }

    assert_eq!(
        crate::families::consolidated::records::object_stream_vertices(&bytes),
        [cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)]
    );
    assert!(crate::families::consolidated::records::object_stream_vertices(&bytes[5..]).is_empty());

    let mut b5 = Vec::new();
    let mut payload = vec![0x05, 0x08, 0x01];
    for value in [90.0f32, 91.0, 92.0] {
        payload.extend_from_slice(&le_f32(value));
    }
    append_b5_record(&mut b5, 0x06, 1, &payload);
    append_b5_record(&mut b5, 0x06, 2, &[]);
    b5.extend_from_slice(&[0x05, 0x08, 0x01]);
    for value in [4.0f32, 5.0, 6.0] {
        b5.extend_from_slice(&le_f32(value));
    }
    assert_eq!(
        crate::families::consolidated::records::object_stream_vertices(&b5),
        [cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0)]
    );
}

#[test]
fn a5_edge_block_parser_groups_two_coparametric_pcurves_and_packet() {
    let blocks =
        crate::families::consolidated::records::consolidated_edge_blocks(&a5_edge_block_stream());
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].co_parametric);
    assert_eq!(blocks[0].pcurves[0].support_id, 0x1234);
    assert_eq!(blocks[0].pcurves[1].range, [0.0, 1.0]);
    assert_eq!(blocks[0].parameters.range, [0.0, 1.0]);
}

#[test]
fn consolidated_edge_block_groups_b_family_pcurves() {
    let blocks =
        crate::families::consolidated::records::consolidated_edge_blocks(&b2_edge_block_stream());
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].co_parametric);
    assert_eq!(blocks[0].pcurves[0].support_id, 0x1234);
    assert_eq!(blocks[0].pcurves[1].range, [0.0, 1.0]);
}

#[test]
fn consolidated_edge_definition_decodes_general_scalar_layout() {
    use crate::families::consolidated::records::ConsolidatedEdgeDefinitionData;

    let mut payload = vec![0x82, 0x05, 0x09, 0x0a, 0x87, 0x0d];
    for value in [0.0_f64, 2.0, 1e-6, 0.5, 1.5, 1.0, -0.5, 1e-6] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x24, &payload),
        Some(ConsolidatedEdgeDefinitionData::Scalar {
            operands: [1, 2, 3463],
            values: vec![0.0, 2.0, 1e-6, 0.5, 1.5, 1.0, -0.5, 1e-6],
        })
    );
    let mut class24_nine_scalars = payload.clone();
    class24_nine_scalars.extend_from_slice(&1e-6_f64.to_le_bytes());
    assert!(
        crate::families::consolidated::records::consolidated_edge_definition_data(
            0x24,
            &class24_nine_scalars
        )
        .is_none()
    );
    payload.pop();
    assert!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x24, &payload)
            .is_none()
    );
}

#[test]
fn consolidated_topology_edge_run_accepts_b_family_pcurves() {
    let runs = crate::families::consolidated::records::consolidated_topology_edge_runs(
        &b2_topology_edge_run_stream(),
    );
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].edge.pcurves[0].support_id, 0x1234);
    assert_eq!(runs[0].node.start_vertex_ref, 889);
    assert_eq!(runs[0].node.end_vertex_ref, 895);
}

#[test]
fn consolidated_native_edge_graph_uses_persistent_endpoint_incidence() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&a5_native_edge_run_stream(3, 10, 11));
    bytes.extend_from_slice(&a5_native_edge_run_stream(6, 11, 12));
    bytes.extend_from_slice(&a5_native_edge_run_stream(9, 12, 10));
    let graph = crate::families::consolidated::records::consolidated_native_edge_graph(&bytes)
        .expect("native edge graph");
    assert_eq!(graph.vertex_identities, [10, 11, 12]);
    assert_eq!(
        graph
            .edges
            .iter()
            .map(|edge| edge.vertices)
            .collect::<Vec<_>>(),
        [[0, 1], [1, 2], [2, 0]]
    );
    assert_eq!(graph.components, [vec![0, 1, 2]]);
    assert!(graph
        .edges
        .iter()
        .all(|edge| edge.run.identity_chain_consistent));
}

#[test]
fn consolidated_native_edge_graph_treats_curve_references_as_run_local() {
    let mut bytes = a5_native_edge_run_stream(3, 10, 11);
    bytes.extend_from_slice(&a5_native_edge_run_stream(3, 20, 21));
    let graph = crate::families::consolidated::records::consolidated_native_edge_graph(&bytes)
        .expect("native edge graph");
    assert_eq!(graph.edges.len(), 2);
    assert_eq!(graph.components, [vec![0], vec![1]]);
}

#[test]
fn a5_edge_block_does_not_cross_an_intervening_framed_record() {
    let mut bytes = a5_pcurve_stream();
    bytes.extend_from_slice(&[0xb2, 0x03, 0x06, 0x01, 0x05, 0x84]);
    bytes.extend_from_slice(&a5_pcurve_stream());
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    assert!(crate::families::consolidated::records::consolidated_edge_blocks(&bytes).is_empty());
}

#[test]
fn a5_edge_binding_resolves_cylinder_by_endpoint_lifts() {
    use crate::families::consolidated::records::ConsolidatedSupportBinding;

    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(
        &a5_cylinder_bound_edge_stream(),
    );
    assert_eq!(blocks.len(), 1);
    assert!(matches!(
        blocks[0].supports[0],
        Some(ConsolidatedSupportBinding::Cylinder { .. })
    ));
    assert!(matches!(
        blocks[0].supports[1],
        Some(ConsolidatedSupportBinding::Cylinder { .. })
    ));
    assert!(blocks[0].endpoint_loci.is_some());
}

#[test]
fn a5_edge_binding_resolves_partner_nurbs_carrier() {
    use crate::families::consolidated::records::ConsolidatedSupportBinding;

    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(
        &a5_nurbs_bound_edge_stream(0.0),
    );
    assert!(matches!(
        blocks[0].supports[0],
        Some(ConsolidatedSupportBinding::Cylinder { .. })
    ));
    assert!(matches!(
        blocks[0].supports[1],
        Some(ConsolidatedSupportBinding::NurbsCarrier { offset, .. }) if offset == 0.0
    ));
    assert_eq!(blocks[0].shared_loci.as_ref().map(Vec::len), Some(2));
    assert!(blocks[0].endpoint_loci.is_some());
}

#[test]
fn a5_edge_binding_resolves_constant_normal_offset_carrier() {
    use crate::families::consolidated::records::ConsolidatedSupportBinding;

    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(
        &a5_nurbs_bound_edge_stream(1.25),
    );
    assert!(matches!(
        blocks[0].supports[1],
        Some(ConsolidatedSupportBinding::NurbsCarrier { offset, .. }) if (offset.abs() - 1.25).abs() < 1e-6
    ));
    assert_eq!(blocks[0].shared_loci.as_ref().map(Vec::len), Some(2));
    assert!(blocks[0].endpoint_loci.is_some());
}

#[test]
fn a5_edge_binding_resolves_circle_by_constant_v_and_arc_range() {
    use crate::families::consolidated::records::ConsolidatedSupportBinding;

    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(
        &a5_circle_bound_edge_stream(),
    );
    assert!(matches!(
        blocks[0].supports[0],
        Some(ConsolidatedSupportBinding::Circle { .. })
    ));
}

#[test]
fn a5_edge_binding_resolves_cone_by_endpoint_lifts() {
    use crate::families::consolidated::records::ConsolidatedSupportBinding;

    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(
        &a5_cone_bound_edge_stream(),
    );
    assert!(matches!(
        blocks[0].supports[0],
        Some(ConsolidatedSupportBinding::Cone { .. })
    ));
    assert!(blocks[0].endpoint_loci.is_some());
}

#[test]
fn consolidated_record_walk_inventory_preserves_width_flag_and_boundaries() {
    use crate::families::consolidated::records::ConsolidatedFamily;

    let first = a6_pcurve_stream();
    let second = b3_cylinder_stream();
    let mut bytes = first.clone();
    bytes.extend_from_slice(&second);
    let records = crate::families::consolidated::records::consolidated_records(&bytes);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].family, ConsolidatedFamily::A);
    assert_eq!(
        (records[0].width, records[0].flag, records[0].class),
        (2, 0x03, 0x20)
    );
    assert_eq!(records[0].range, 0..first.len());
    assert_eq!(records[1].family, ConsolidatedFamily::B);
    assert_eq!(records[1].range, first.len()..first.len() + second.len());
}

#[test]
fn consolidated_record_walk_suppresses_payload_records_and_resumes_after_parent() {
    let nested = [0xb2, 0x03, 0x20, 1, 7, 0xaa];
    let mut outer = vec![0xb2, 0x03, 0x20, nested.len() as u8, 1];
    outer.extend_from_slice(&nested);
    let sibling_start = outer.len();
    outer.extend_from_slice(&[0xb2, 0x03, 0x20, 1, 2, 0xbb]);

    let records = crate::families::consolidated::records::consolidated_records(&outer);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].range, 0..sibling_start);
    assert_eq!(records[1].range, sibling_start..outer.len());
}
