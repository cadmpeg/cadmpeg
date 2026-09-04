// SPDX-License-Identifier: Apache-2.0
//! Record-decoder tests for the `consolidated` family over synthetic byte fixtures.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::CatiaCodec;

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
fn consolidated_edge_block_does_not_cross_an_unframed_gap() {
    let source = a5_edge_block_stream();
    let records = crate::wire::records::consolidated_records(&source);
    assert_eq!(records.len(), 3);
    let split = records[0].range.end;
    let mut bytes = source[..split].to_vec();
    bytes.push(0);
    bytes.extend_from_slice(&source[split..]);

    assert!(
        crate::families::consolidated::records::consolidated_edge_blocks(&bytes).is_empty(),
        "a multi-frame edge block must not cross an unframed gap"
    );
}

#[test]
fn indexed_resolver_matches_the_one_shot_resolver_identity() {
    let bytes = a5_cylinder_bound_edge_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    let signature =
        |blocks: &[crate::families::consolidated::records::ResolvedConsolidatedEdgeBlock]| {
            blocks
                .iter()
                .map(|block| {
                    (
                        block.block.pcurves[0].pos,
                        block.block.pcurves[1].pos,
                        block.block.parameters.pos,
                        block.supports.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
    let one_shot = crate::families::consolidated::records::resolve_consolidated_edge_blocks(&bytes);
    let indexed =
        crate::families::consolidated::records::resolve_consolidated_edge_blocks_from_records(
            &bytes, &records,
        );
    assert_eq!(signature(&indexed), signature(&one_shot));
}

#[test]
fn consolidated_edge_definition_decodes_general_scalar_layout() {
    use crate::families::consolidated::records::ConsolidatedEdgeDefinitionData;

    let mut payload = vec![0x82, 0x05, 0x09, 0x0a, 0x87, 0x0d];
    for value in [0.0_f64, 2.0, 1.0e-6, 0.5, 1.5, 1.0, -0.5, 1.0e-6] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x24, &payload),
        Some(ConsolidatedEdgeDefinitionData::Scalar {
            operands: [1, 2, 3463],
            values: vec![0.0, 2.0, 1.0e-6, 0.5, 1.5, 1.0, -0.5, 1.0e-6],
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
fn class23_nine_scalar_definition_requires_three_equal_triples() {
    use crate::families::consolidated::records::ConsolidatedEdgeDefinitionData;

    let mut payload = vec![0x82, 0x05, 0x09, 0x0a, 0x87, 0x0d];
    for value in [0.0_f64, 2.0, 1.0, 0.0, 2.0, 1.0, 0.0, 2.0, 1.0] {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x23, &payload),
        Some(ConsolidatedEdgeDefinitionData::Scalar {
            operands: [1, 2, 3463],
            values: vec![0.0, 2.0, 1.0, 0.0, 2.0, 1.0, 0.0, 2.0, 1.0],
        })
    );

    let mut unequal_tolerances = payload;
    for offset in [6 + 2 * 8, 6 + 8 * 8] {
        unequal_tolerances[offset..offset + 8].copy_from_slice(&2.0_f64.to_le_bytes());
    }
    assert!(
        crate::families::consolidated::records::consolidated_edge_definition_data(
            0x23,
            &unequal_tolerances
        )
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
fn b2_edge_binding_resolves_direction_bearing_plane_by_endpoint_lifts() {
    use crate::families::consolidated::records::ConsolidatedSupportBinding;

    let plane_stream = b2_plane_carrier_stream();
    let plane_end = crate::families::b2::records::b2_plane_carriers(&plane_stream)[0].end;
    let mut bytes = b2_edge_block_stream();
    bytes.extend_from_slice(&plane_stream[..plane_end]);
    for point in [[10.0f32, 20.0, 0.0], [11.0, 20.0, 1.0]] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&le_f32(value));
        }
    }

    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(&bytes);
    assert_eq!(blocks.len(), 1);
    assert!(matches!(
        blocks[0].supports[0],
        Some(ConsolidatedSupportBinding::Plane { .. })
    ));
    assert!(matches!(
        blocks[0].supports[1],
        Some(ConsolidatedSupportBinding::Plane { .. })
    ));
    assert_eq!(
        blocks[0].endpoint_loci,
        Some([
            cadmpeg_ir::math::Point3::new(10.0, 20.0, 0.0),
            cadmpeg_ir::math::Point3::new(11.0, 20.0, 1.0),
        ])
    );
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
        Some(ConsolidatedSupportBinding::NurbsCarrier { offset, .. }) if (offset.abs() - 1.25).abs() < 1.0e-6
    ));
    assert_eq!(blocks[0].shared_loci.as_ref().map(Vec::len), Some(2));
    assert!(blocks[0].endpoint_loci.is_some());
}

#[test]
fn a5_edge_binding_jointly_resolves_two_direct_nurbs_carriers() {
    use crate::families::consolidated::records::ConsolidatedSupportBinding;

    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(
        &a5_nurbs_pair_bound_edge_stream(false),
    );
    assert_eq!(blocks.len(), 1);
    assert!(
        blocks[0].supports.iter().all(|support| {
            matches!(
                support,
                Some(ConsolidatedSupportBinding::NurbsCarrier { offset, .. }) if *offset == 0.0
            )
        }),
        "{:#?}",
        blocks[0].supports
    );
    assert_eq!(blocks[0].shared_loci.as_ref().map(Vec::len), Some(2));
    assert!(blocks[0].endpoint_loci.is_some());
}

#[test]
fn a5_edge_binding_rejects_nonunique_direct_nurbs_carrier_pairs() {
    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(
        &a5_nurbs_pair_bound_edge_stream(true),
    );
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].supports, [None, None]);
    assert!(blocks[0].shared_loci.is_none());
    assert!(blocks[0].endpoint_loci.is_none());
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
fn a5_edge_binding_uses_circle_identity_to_break_geometric_ties() {
    use crate::families::consolidated::records::ConsolidatedSupportBinding;

    let mut bytes = a5_circle_bound_edge_stream();
    let original_circle_offset = bytes.len() - b2_circle_stream().len();
    let mut duplicate = b2_circle_stream();
    duplicate[6..8].copy_from_slice(&0x1235_u16.to_le_bytes());
    bytes.extend_from_slice(&duplicate);

    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(&bytes);
    assert!(matches!(
        blocks[0].supports[0],
        Some(ConsolidatedSupportBinding::Circle { pos }) if pos == original_circle_offset
    ));
}

#[test]
fn a5_edge_binding_rejects_duplicate_circle_identities() {
    let mut bytes = a5_circle_bound_edge_stream();
    bytes.extend_from_slice(&b2_circle_stream());

    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(&bytes);
    assert!(blocks[0].supports[0].is_none());
}

#[test]
fn a5_edge_binding_rejects_an_identity_with_a_conflicting_circle_chart() {
    let mut bytes = a5_circle_bound_edge_stream();
    let original_circle_offset = bytes.len() - b2_circle_stream().len();
    bytes[original_circle_offset + 6..original_circle_offset + 8]
        .copy_from_slice(&0x1235_u16.to_le_bytes());
    let mut conflicting = b2_circle_stream();
    conflicting[40..48].copy_from_slice(&1.0_f64.to_le_bytes());
    bytes.extend_from_slice(&conflicting);

    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(&bytes);
    assert!(blocks[0].supports[0].is_none());
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
fn a5_edge_binding_resolves_torus_by_scaled_chart_endpoint_lifts() {
    use crate::families::consolidated::records::ConsolidatedSupportBinding;

    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(
        &a5_torus_bound_edge_stream(),
    );
    assert!(blocks[0]
        .supports
        .iter()
        .all(|support| matches!(support, Some(ConsolidatedSupportBinding::Torus { .. }))));
    assert_eq!(blocks[0].shared_loci.as_ref().map(Vec::len), Some(2));
    let endpoints = blocks[0].endpoint_loci.expect("lifted torus endpoints");
    assert!(endpoints[0].distance_squared(cadmpeg_ir::math::Point3::new(1.0, 11.0, 3.0)) < 1e-24);
    assert!(endpoints[1].distance_squared(cadmpeg_ir::math::Point3::new(-8.0, 2.0, 3.0)) < 1e-24);
}

#[test]
fn a5_edge_binding_resolves_sphere_by_endpoint_lifts() {
    use crate::families::consolidated::records::ConsolidatedSupportBinding;

    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(
        &crate::test_support::a5_sphere_bound_edge_stream(),
    );
    assert!(blocks[0]
        .supports
        .iter()
        .all(|support| matches!(support, Some(ConsolidatedSupportBinding::Sphere { .. }))));
    assert_eq!(blocks[0].shared_loci.as_ref().map(Vec::len), Some(2));
    let endpoints = blocks[0].endpoint_loci.expect("lifted sphere endpoints");
    assert!(endpoints[0].distance_squared(cadmpeg_ir::math::Point3::new(6.0, 2.0, 3.0)) < 1e-24);
    assert!(endpoints[1].distance_squared(cadmpeg_ir::math::Point3::new(1.0, 7.0, 3.0)) < 1e-24);
}

#[test]
fn a5_edge_binding_rejects_duplicate_sphere_endpoint_lifts() {
    let mut bytes = crate::test_support::a5_sphere_bound_edge_stream();
    bytes.extend_from_slice(&crate::test_support::b2_sphere_stream());

    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(&bytes);
    assert_eq!(blocks[0].supports, [None, None]);
}

#[test]
fn a5_edge_binding_rejects_duplicate_torus_endpoint_lifts() {
    let mut bytes = a5_torus_bound_edge_stream();
    bytes.extend_from_slice(&crate::test_support::b2_torus_stream());

    let blocks = crate::families::consolidated::records::resolve_consolidated_edge_blocks(&bytes);
    assert_eq!(blocks[0].supports, [None, None]);
}

#[test]
fn consolidated_record_walk_inventory_preserves_width_flag_and_boundaries() {
    use crate::wire::records::ConsolidatedFamily;

    let first = a6_pcurve_stream();
    let second = b3_cylinder_stream();
    let mut bytes = first.clone();
    bytes.extend_from_slice(&second);
    let records = crate::wire::records::consolidated_records(&bytes);
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

    let records = crate::wire::records::consolidated_records(&outer);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].range, 0..sibling_start);
    assert_eq!(records[1].range, sibling_start..outer.len());
}

#[test]
fn consolidated_support_resolution_withholds_cross_family_matches() {
    let mut bytes = a5_cone_bound_edge_stream();
    let cylinder = crate::families::b2::records::b2_cylinders(&b2_cylinder_stream())
        .into_iter()
        .next()
        .expect("one cylinder carrier");
    bytes.extend_from_slice(&b2_cylinder_stream());
    for uv in [[0.0, 2.0], [1.0, 3.0]] {
        let point = crate::families::b2::records::b2_cylinder_point(&cylinder, uv)
            .expect("cylinder endpoint");
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in [point.x, point.y, point.z] {
            bytes.extend_from_slice(&(value as f32).to_le_bytes());
        }
    }

    let resolved = crate::families::consolidated::records::resolve_consolidated_edge_blocks(&bytes);
    let [edge] = resolved.as_slice() else {
        panic!("one consolidated edge block");
    };
    assert_eq!(edge.supports, [None, None]);
    assert!(edge.shared_loci.is_none());
}

#[test]
fn consolidated_support_identity_mismatch_does_not_fall_back_to_geometry() {
    let mut bytes = a5_cone_bound_edge_stream();
    bytes.extend_from_slice(&b2_embedded_cylinder_stream_with_object_id(0x1234));

    let resolved = crate::families::consolidated::records::resolve_consolidated_edge_blocks(&bytes);
    let [edge] = resolved.as_slice() else {
        panic!("one consolidated edge block");
    };
    assert_eq!(edge.supports, [None, None]);
}

#[test]
fn consolidated_edge_use_run_is_independent_of_pcurve_availability() {
    use crate::families::b2::records::B2UseSense;

    let runs = crate::families::consolidated::records::consolidated_edge_use_runs(
        &a5_native_edge_identity_stream(6, 139, 142),
    );
    let [run] = runs.as_slice() else {
        panic!("one standalone edge-use run");
    };
    assert!(run.identity_chain_consistent);
    assert_eq!(run.uses[0].sense, Some(B2UseSense::Sense88));
    assert_eq!(run.uses[1].sense, Some(B2UseSense::Sense84));
    assert_eq!(run.node.start_vertex_ref, 139);
    assert_eq!(run.node.end_vertex_ref, 142);
}

#[test]
fn consolidated_edge_use_run_owns_adjacent_compact_definition() {
    use crate::families::consolidated::records::ConsolidatedEdgeDefinitionData;

    let mut bytes = vec![0xb2, 0x03, 0x24, 0x04, 0x05, 0x81, 0x05, 0x0f, 0x87];
    bytes.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));

    let runs = crate::families::consolidated::records::consolidated_edge_use_runs(&bytes);
    let [run] = runs.as_slice() else {
        panic!("one edge-use run");
    };
    let definition = run.definition.as_ref().expect("adjacent definition");
    assert_eq!(definition.class, 0x24);
    assert_eq!(definition.header_token, 5);
    assert_eq!(definition.payload, [0x81, 0x05, 0x0f, 0x87]);
    assert_eq!(
        definition.data,
        Some(ConsolidatedEdgeDefinitionData::Compact24 { operand: 1 })
    );

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(
        native.consolidated_edge_nodes[0]
            .definition
            .as_ref()
            .expect("native definition")
            .class,
        0x24
    );
    assert!(matches!(
        native.consolidated_edge_nodes[0]
            .definition
            .as_ref()
            .and_then(|definition| definition.data.as_ref()),
        Some(
            crate::families::consolidated::records::ConsolidatedEdgeDefinitionData::Compact24 {
                operand: 1
            }
        )
    ));
}

#[test]
fn consolidated_edge_use_run_accepts_compact_successor_layout() {
    use crate::families::b2::records::B2UseSense;
    use crate::families::consolidated::records::ConsolidatedEdgeDefinitionData;

    let bytes = [
        0xb2, 0x03, 0x5e, 0x06, 0x05, 0x03, 0x09, 0x0f, 0x07, 0x0b, 0x21, 0xb2, 0x03, 0x24, 0x04,
        0x05, 0x81, 0x29, 0x0f, 0x87, 0xb2, 0x03, 0x06, 0x04, 0x05, 0x82, 0x05, 0x2d, 0x88, 0xb2,
        0x03, 0x06, 0x04, 0x05, 0x82, 0x09, 0x31, 0x84,
    ];
    let runs = crate::families::consolidated::records::consolidated_edge_use_runs(&bytes);
    let [run] = runs.as_slice() else {
        panic!("one successor-layout edge run")
    };
    assert!(run.identity_chain_consistent);
    assert_eq!(run.uses[0].sense, Some(B2UseSense::Sense88));
    assert_eq!(run.uses[1].sense, Some(B2UseSense::Sense84));
    assert_eq!(run.uses[0].references.as_deref(), Some(&[1, 11][..]));
    assert_eq!(run.uses[1].references.as_deref(), Some(&[2, 12][..]));
    assert_eq!(
        run.definition.as_ref().and_then(|value| value.data.clone()),
        Some(ConsolidatedEdgeDefinitionData::Compact24 { operand: 10 })
    );
}

#[test]
fn compact_owner_ordinal_selects_the_owned_edge_node() {
    let bytes = [
        0xb2, 0x03, 0x5f, 0x04, 0x05, 0x82, 0x1d, 0x03, 0x05, 0xb2, 0x03, 0x62, 0x08, 0x05, 0x82,
        0x0b, 0x21, 0x84, 0x41, 0xff, 0x0f, 0x01, 0xb2, 0x03, 0x5d, 0x02, 0x05, 0x03, 0x00, 0xb2,
        0x03, 0x05, 0x03, 0x05, 0x82, 0x0b, 0x57, 0xb2, 0x03, 0x5e, 0x06, 0x05, 0x03, 0x09, 0x0f,
        0x07, 0x0b, 0x21,
    ];
    let records = crate::wire::records::consolidated_records(&bytes);
    let owned = crate::families::consolidated::records::consolidated_owned_edge_nodes_from_records(
        &bytes, &records,
    );
    let [owned] = owned.as_slice() else {
        panic!("one owner-selected edge node")
    };
    assert_eq!(owned.owner_pos, 9);
    assert_eq!(owned.allocation_ordinal, 2);
    assert_eq!(owned.node.pos, 37);
}

#[test]
fn compact_endpoint_walk_resolves_children_and_backward_edge_links() {
    let first_edge = [
        0xb2, 0x03, 0x5e, 0x09, 0x05, 0x06, 0x20, 0x03, 0x07, 0x06, 0x30, 0x06, 0x31, 0x21,
    ];
    let vertex = [0xb2, 0x03, 0x5d, 0x02, 0x05, 0x03, 0x00];
    let second_edge = [
        0xb2, 0x03, 0x5e, 0x09, 0x05, 0x06, 0x21, 0x09, 0x0d, 0x06, 0x32, 0x06, 0x33, 0x21,
    ];
    let first_vertex_pos = first_edge.len();
    let second_vertex_pos = first_vertex_pos + vertex.len();
    let mut bytes = first_edge.to_vec();
    bytes.extend_from_slice(&vertex);
    bytes.extend_from_slice(&vertex);
    bytes.extend_from_slice(&second_edge);
    let records = crate::wire::records::consolidated_records(&bytes);
    let endpoints =
        crate::families::consolidated::records::consolidated_compact_edge_endpoints_from_records(
            &bytes, &records,
        );

    assert_eq!(endpoints.len(), 2);
    assert_eq!(
        endpoints[0].endpoint_records,
        [first_vertex_pos, second_vertex_pos]
    );
    assert_eq!(
        endpoints[1].endpoint_records,
        [first_vertex_pos, second_vertex_pos]
    );
}

#[test]
fn width_coded_endpoint_distances_resolve_forward_class18_records() {
    let edge = [
        0xb2, 0x03, 0x5e, 0x0a, 0x05, 0x03, 0x08, 0x02, 0x00, 0x08, 0x03, 0x00, 0x07, 0x0b, 0x21,
    ];
    let filler = [0xb2, 0x03, 0x05, 0x01, 0x05, 0x01];
    let endpoint = [0xb2, 0x03, 0x18, 0x01, 0x05, 0x01];
    let first_endpoint = edge.len() + filler.len();
    let second_endpoint = first_endpoint + endpoint.len();
    let mut bytes = edge.to_vec();
    bytes.extend_from_slice(&filler);
    bytes.extend_from_slice(&endpoint);
    bytes.extend_from_slice(&endpoint);
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_eq!(
        records
            .iter()
            .map(|record| record.class)
            .collect::<Vec<_>>(),
        [0x5e, 0x05, 0x18, 0x18]
    );
    let nodes = crate::families::b2::records::b2_edge_nodes_from_records(&bytes, &records);
    assert_eq!(nodes.len(), 1);
    assert_eq!([nodes[0].start_vertex_ref, nodes[0].end_vertex_ref], [2, 3]);
    let endpoints =
        crate::families::consolidated::records::consolidated_compact_edge_endpoints_from_records(
            &bytes, &records,
        );

    let [resolved] = endpoints.as_slice() else {
        panic!("one edge with two forward endpoint records")
    };
    assert_eq!(resolved.endpoint_records, [first_endpoint, second_endpoint]);

    let split_sources = crate::wire::records::consolidated_records_in_ranges(
        &bytes,
        [0..edge.len(), edge.len()..bytes.len()],
    );
    assert!(
        crate::families::consolidated::records::consolidated_compact_edge_endpoints_from_records(
            &bytes,
            &split_sources,
        )
        .is_empty(),
        "a forward endpoint walk cannot cross bounded record sources"
    );

    let mut reordered = endpoint.to_vec();
    let edge_pos = reordered.len();
    reordered.extend_from_slice(&edge);
    let filler_pos = reordered.len();
    reordered.extend_from_slice(&filler);
    let second_endpoint_pos = reordered.len();
    reordered.extend_from_slice(&endpoint);
    let records = crate::wire::records::consolidated_records_in_sources(
        &reordered,
        [[
            edge_pos..filler_pos,
            filler_pos..second_endpoint_pos,
            0..endpoint.len(),
            second_endpoint_pos..reordered.len(),
        ]],
    );
    let endpoints =
        crate::families::consolidated::records::consolidated_compact_edge_endpoints_from_records(
            &reordered, &records,
        );
    let [resolved] = endpoints.as_slice() else {
        panic!("one edge can walk across physical extents in logical source order")
    };
    assert_eq!(resolved.endpoint_records, [0, second_endpoint_pos]);

    let mut spanning = edge.to_vec();
    let filler_start = spanning.len();
    spanning.extend_from_slice(&[0xa5, 0x03, 0x34]);
    spanning.extend_from_slice(&8u32.to_le_bytes());
    spanning.extend_from_slice(&[0x05, 0, 1, 2, 3, 4, 5, 6, 7]);
    let spanning_first_endpoint = spanning.len();
    spanning.extend_from_slice(&endpoint);
    let spanning_second_endpoint = spanning.len();
    spanning.extend_from_slice(&endpoint);
    let split = filler_start + 10;
    let records = crate::wire::records::consolidated_records_in_sources(
        &spanning,
        [[0..split, split..spanning.len()]],
    );
    assert!(!records[1].physically_contiguous);
    let endpoints =
        crate::families::consolidated::records::consolidated_compact_edge_endpoints_from_records(
            &spanning, &records,
        );
    let [resolved] = endpoints.as_slice() else {
        panic!("a spanning frame remains in forward-distance ordinal accounting")
    };
    assert_eq!(
        resolved.endpoint_records,
        [spanning_first_endpoint, spanning_second_endpoint]
    );

    let mut wrong_class = bytes;
    wrong_class[first_endpoint + 2] = 0x19;
    let records = crate::wire::records::consolidated_records(&wrong_class);
    assert!(
        crate::families::consolidated::records::consolidated_compact_edge_endpoints_from_records(
            &wrong_class,
            &records,
        )
        .is_empty()
    );
}

#[test]
fn fixed_owner_boundary_cycle_rejects_cross_source_endpoint_network() {
    let (bytes, _, _, endpoint_records) = b2_fixed_owner_boundary_cycle_stream();
    let records = crate::wire::records::consolidated_records(&bytes);
    assert_eq!(
        crate::families::consolidated::records::consolidated_owner_boundary_cycles_from_records(
            &bytes, &records,
        )
        .len(),
        1
    );

    let split = endpoint_records[1][1];
    let split_records = crate::wire::records::consolidated_records_in_ranges(
        &bytes,
        [0..split, split..bytes.len()],
    );
    assert!(
        crate::families::consolidated::records::consolidated_owner_boundary_cycles_from_records(
            &bytes,
            &split_records,
        )
        .is_empty(),
        "a fixed-owner cycle cannot join endpoint records across bounded sources"
    );
}

#[test]
fn consolidated_edge_definition_decodes_class25_scalar_layouts() {
    use crate::families::consolidated::records::ConsolidatedEdgeDefinitionData;

    let operands = [0x82, 0x05, 0xe7, 0x0a, 0x87, 0x0d];
    let mut plain = operands.to_vec();
    for value in [1.0_f64, 2.0, 1.0e-6, 3.0, 4.0, 1.0, 5.0, 1.0e-6] {
        plain.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &plain),
        Some(ConsolidatedEdgeDefinitionData::Scalar25 {
            operands: [1, 57, 3463],
            persistent_lead: Some(0x0a),
            values: vec![1.0, 2.0, 1.0e-6, 3.0, 4.0, 1.0, 5.0, 1.0e-6],
        })
    );

    let mut segmented = operands.to_vec();
    for value in [1.0_f64, 2.0, 1.0e-6, 3.0, 4.0] {
        segmented.extend_from_slice(&value.to_le_bytes());
    }
    segmented.push(0x82);
    for value in [1.0_f64, 2.0, 3.0, 4.0, 5.0, 1.0e-6] {
        segmented.extend_from_slice(&value.to_le_bytes());
    }
    assert!(matches!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &segmented),
        Some(ConsolidatedEdgeDefinitionData::SegmentedScalar25 {
            operands: [1, 57, 3463],
            persistent_lead: Some(0x0a),
            marker: 0x82,
            ref trailing,
            ..
        }) if trailing.len() == 6
    ));
    segmented[46] = 0x84;
    assert!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &segmented)
            .is_none()
    );

    let mut odd_lead = plain.clone();
    odd_lead[3] = 0x0b;
    odd_lead.drain(odd_lead.len() - 8..);
    assert!(matches!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &odd_lead),
        Some(ConsolidatedEdgeDefinitionData::Scalar25 {
            persistent_lead: Some(0x0b),
            ref values,
            ..
        }) if values.len() == 7
    ));

    let mut long_segment = operands.to_vec();
    for value in [1.0_f64, 2.0, 1.0e-6, 3.0, 4.0] {
        long_segment.extend_from_slice(&value.to_le_bytes());
    }
    long_segment.push(0x89);
    for value in 0..20 {
        long_segment.extend_from_slice(&f64::from(value).to_le_bytes());
    }
    assert!(matches!(
        crate::families::consolidated::records::consolidated_edge_definition_data(0x25, &long_segment),
        Some(ConsolidatedEdgeDefinitionData::SegmentedScalar25 {
            marker: 0x89,
            ref trailing,
            ..
        }) if trailing.len() == 20
    ));

    let mut bytes = vec![0xb2, 0x03, 0x25, plain.len() as u8, 0x05];
    bytes.extend_from_slice(&plain);
    bytes.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));
    let native = crate::native::CatiaNative::decode(&bytes);
    assert!(matches!(
        native.consolidated_edge_nodes[0]
            .definition
            .as_ref()
            .and_then(|definition| definition.data.as_ref()),
        Some(
            crate::families::consolidated::records::ConsolidatedEdgeDefinitionData::Scalar25 {
                operands: [1, 57, 3463],
                persistent_lead: Some(0x0a),
                ..
            }
        )
    ));

    let mut descriptor_payload = vec![0x08, 0x34, 0x12, 0x02];
    descriptor_payload.extend_from_slice(&3.0_f64.to_le_bytes());
    descriptor_payload.extend_from_slice(&7.0_f64.to_le_bytes());
    let mut described = vec![0xb2, 0x03, 0x18, descriptor_payload.len() as u8, 0x05];
    described.extend_from_slice(&descriptor_payload);
    described.extend_from_slice(&bytes);
    let runs = crate::families::consolidated::records::consolidated_class25_edge_runs(&described);
    let [run] = runs.as_slice() else {
        panic!("one described class-25 edge run");
    };
    assert_eq!(run.descriptor.record_id, 0x1234);
    assert_eq!(run.descriptor.values, [3.0, 7.0]);
    assert!(run.identity_chain_consistent);
    let native = crate::native::CatiaNative::decode(&described);
    assert_eq!(
        native.consolidated_edge_nodes[0]
            .class25_descriptor
            .as_ref()
            .expect("native class-25 descriptor")
            .control,
        0x02
    );
}

#[test]
fn consolidated_analytic_circle_run_binds_adjacent_carrier() {
    fn record(class: u8, token: u8, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0xb2, 0x03, class, payload.len() as u8, token];
        bytes.extend_from_slice(payload);
        bytes
    }

    let mut parameter = vec![0x05, 0x00];
    parameter.extend_from_slice(&12.0_f64.to_le_bytes());
    parameter.extend_from_slice(&34.0_f64.to_le_bytes());
    let mut circle = vec![0x05];
    for value in [12.0_f64, 34.0, 5.0, 0.0, 10.0] {
        circle.extend_from_slice(&value.to_le_bytes());
    }
    circle.push(0x01);
    circle.extend_from_slice(&0.0_f64.to_le_bytes());
    let mut definition = vec![0x82, 0x05, 0x09, 0x0a, 0x87, 0x0d];
    for value in [0.0_f64, 10.0, 1.0e-6, 4.0, 9.0, 1.0, -2.0, 1.0e-6] {
        definition.extend_from_slice(&value.to_le_bytes());
    }
    let mut bytes = record(0x18, 0x15, &parameter);
    bytes.extend_from_slice(&record(0x19, 0x05, &circle));
    bytes.extend_from_slice(&record(0x23, 0x05, &definition));
    bytes.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));

    let runs =
        crate::families::consolidated::records::consolidated_analytic_circle_edge_runs(&bytes);
    let [run] = runs.as_slice() else {
        panic!("one analytic-circle edge run");
    };
    assert_eq!(run.circle.center_pair, [12.0, 34.0]);
    assert_eq!(run.circle.radius, 5.0);
    assert_eq!(run.descriptor.header_token, 0x15);
    assert_eq!(run.definition.pos, parameter.len() + circle.len() + 10);
    assert!(run.identity_chain_consistent);

    let native = crate::native::CatiaNative::decode(&bytes);
    let binding = native.consolidated_edge_nodes[0]
        .analytic_circle
        .as_ref()
        .expect("native analytic circle");
    assert_eq!(binding.circle, "catia:consolidated:circle#0");
    assert_eq!(native.consolidated_circles[0].center_pair, [12.0, 34.0]);
    assert_eq!(native.consolidated_circles[0].range, [0.0, 10.0]);

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store analytic circle binding");
    assert_eq!(
        crate::native::CatiaNative::load(&namespace).expect("load analytic circle binding"),
        native
    );

    let mut invalid = native;
    invalid.consolidated_edge_nodes[0]
        .analytic_circle
        .as_mut()
        .expect("analytic circle binding")
        .circle = "missing".to_string();
    let mut invalid_namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    invalid
        .store(&mut invalid_namespace)
        .expect("store invalid analytic circle binding");
    assert!(crate::native::CatiaNative::load(&invalid_namespace).is_err());

    let circle_end = parameter.len() + circle.len() + 10;
    let mut broken = bytes[..circle_end].to_vec();
    broken.extend_from_slice(&record(0x05, 0x05, &[0x00]));
    broken.extend_from_slice(&bytes[circle_end..]);
    assert!(
        crate::families::consolidated::records::consolidated_analytic_circle_edge_runs(&broken)
            .is_empty()
    );
}

#[test]
fn a5_topology_edge_run_preserves_uses_and_native_endpoint_identities() {
    use crate::families::b2::records::B2UseSense;

    let runs = crate::families::consolidated::records::consolidated_topology_edge_runs(
        &a5_topology_edge_run_stream(),
    );
    assert_eq!(runs.len(), 1);
    assert!(runs[0].edge.co_parametric);
    assert_eq!(runs[0].uses[0].sense, Some(B2UseSense::Sense84));
    assert_eq!(runs[0].uses[1].sense, Some(B2UseSense::Sense88));
    assert_eq!(runs[0].uses[0].references.as_deref(), Some(&[1, 2][..]));
    assert_eq!(runs[0].uses[1].references.as_deref(), Some(&[2, 3][..]));
    assert!(!runs[0].identity_chain_consistent);
    assert_eq!(runs[0].node.start_vertex_ref, 889);
    assert_eq!(runs[0].node.end_vertex_ref, 895);
}

#[test]
fn decode_transfers_exact_consolidated_line_profiles() {
    let mut file = standard_catpart();
    file.splice(16..16, b2_line_profile_stream());
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode consolidated line profile");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONSOLIDATED_LINE_PROFILE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CONSOLIDATED_LINE_PROFILE_COUNT),
        1
    );
    assert!(decoded.ir().model.curves.iter().any(|curve| matches!(
        curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Line { origin, direction }
            if origin == cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0)
                && direction == cadmpeg_ir::math::Vector3::new(0.0, 0.6, 0.8)
    )));
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("consolidated line-profile record(s)")));
}

#[test]
fn decode_routes_a_line_profile_only_nested_stream_to_a_wire() {
    let file = standard_catpart_from_streams(&b2_line_profile_stream(), &[]);
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode line-profile-only nested stream");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CONSOLIDATED_LINE_PROFILE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(cadmpeg_ir::CoverageKey::new(
                "attached_standalone_wire_edge_count"
            )),
        1
    );
    assert_eq!(decoded.ir().model.edges[0].param_range, Some([-4.0, 9.0]));
    assert_eq!(
        decoded.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn decode_routes_a_resolved_revolution_only_nested_stream_to_freeform() {
    let file = standard_catpart_from_streams(&b2_resolved_revolution_stream(), &[]);
    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode revolution-only nested stream");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::TRANSFERRED_CONSOLIDATED_REVOLUTION_COUNT),
        1
    );
    let revolution = decoded
        .ir()
        .model
        .procedural_surfaces
        .iter()
        .find(|surface| surface.id.0 == "catia:consolidated:surface-revolution#0")
        .expect("transferred freeform revolution");
    assert!(matches!(
        revolution.definition(),
        cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Revolution {
            parameter_interval: Some([-4.0, 9.0]),
            ..
        }
    ));
    assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
}

#[test]
fn transferred_line_profile_identities_retain_their_native_ordinals() {
    let mut file = standard_catpart();
    file.splice(
        16..16,
        [b2_line_profile_stream(), b2_line_profile_stream()].concat(),
    );
    let file_len = u32::try_from(file.len()).expect("bounded CATPart fixture");
    file[8..12].copy_from_slice(&be32(file_len));

    let decoded = CatiaCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("decode mixed-metric line profiles");
    let line_ids = decoded
        .ir()
        .model
        .curves
        .iter()
        .filter(|curve| {
            curve
                .id
                .0
                .starts_with("catia:consolidated:line-profile-curve#")
        })
        .map(|curve| curve.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        line_ids,
        [
            "catia:consolidated:line-profile-curve#0",
            "catia:consolidated:line-profile-curve#1",
        ]
    );
}
