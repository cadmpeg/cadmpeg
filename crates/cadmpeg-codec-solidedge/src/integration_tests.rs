// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, InspectOptions};
use cadmpeg_ir::codec::{Codec, Confidence, DecodeOptions};

use crate::{test_support, SolidEdgeCodec};

const EPS_SPHERE_RADIUS_MM: f64 = 1.0e-12;
const EPS_RESOLUTION_MM: f64 = 1.0e-15;
const EPS_RING_VERTEX_MM: f64 = 1.0e-9;

#[test]
fn directory_signature_detects_solid_edge() {
    let bytes = test_support::compound();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert_eq!(SolidEdgeCodec.detect(&ctx, root).unwrap(), Confidence::High);
    ctx.finish_session().unwrap();
}

#[test]
fn generic_compound_file_has_no_solid_edge_identity() {
    let bytes = cadmpeg_test_support::compound::compound_fixture();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert_eq!(SolidEdgeCodec.detect(&ctx, root).unwrap(), Confidence::No);
    ctx.finish_session().unwrap();
    assert!(SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .is_err());
}

#[test]
fn inspection_lists_compound_streams() {
    let bytes = test_support::compound();
    let summary = SolidEdgeCodec
        .inspect(&mut Cursor::new(bytes), &InspectOptions::default())
        .unwrap();
    assert_eq!(summary.entries.len(), 2);
}

#[test]
fn full_decode_reports_pending_bodies() {
    let bytes = test_support::compound();
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().geometry.bodies.is_empty());
    for code in [
        crate::loss::SolidEdgeLossCode::BodiesPending,
        crate::loss::SolidEdgeLossCode::FeaturesPending,
        crate::loss::SolidEdgeLossCode::AttributesPending,
    ] {
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == code.note("").code));
    }
}

#[test]
fn primary_partition_projects_an_exact_sphere_in_millimetres() {
    let bytes = test_support::with_partitions(&[test_support::sphere_partition()]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert!(result.report().geometry_transferred());
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies.len(), 1);
    assert_eq!(ir.geometry.faces.len(), 1);
    assert_eq!(ir.geometry.regions.len(), 1);
    assert_eq!(ir.geometry.shells.len(), 1);
    assert_eq!(
        ir.geometry.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Solid
    );
    let cadmpeg_ir::geometry::SurfaceGeometry::Solved(
        cadmpeg_ir::geometry::SolvedSurfaceGeometry::Sphere(sphere),
    ) = &ir.geometry.surfaces[0].geometry
    else {
        panic!("sphere carrier");
    };
    assert!((sphere.radius().get() - 7.0).abs() < EPS_SPHERE_RADIUS_MM);
    assert_geometry_valid(ir);
}

#[test]
fn unreadable_partition_preserves_another_partition() {
    let bytes =
        test_support::with_partitions(&[b"broken".to_vec(), test_support::sphere_partition()]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::SolidEdgeLossCode::PartitionUnreadable
            .note("")
            .code));
}

#[test]
fn partition_indices_have_separate_identity_spaces() {
    let partition = test_support::sphere_partition();
    let bytes = test_support::with_partitions(&[partition.clone(), partition]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies.len(), 2);
    assert_ne!(ir.geometry.bodies[0].id, ir.geometry.bodies[1].id);
    assert_geometry_valid(ir);
}

#[test]
fn missing_terminator_keeps_framed_body_nodes() {
    let mut partition = test_support::sphere_partition();
    partition.truncate(partition.len() - 4);
    let bytes = test_support::with_partitions(&[partition]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::SolidEdgeLossCode::PartitionIncomplete
            .note("")
            .code));
}

#[test]
fn carrier_reference_order_preserves_a_closed_cube() {
    let bytes = test_support::with_partitions(&[test_support::cube_partition()]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies.len(), 1);
    assert_eq!(ir.geometry.faces.len(), 6);
    assert_eq!(ir.geometry.edges.len(), 12);
    assert_eq!(ir.geometry.vertices.len(), 8);
    assert_eq!(ir.geometry.coedges.len(), 24);
    assert_geometry_valid(ir);
    assert!(!result.report().losses.iter().any(|loss| loss.code
        == crate::loss::SolidEdgeLossCode::PartitionUnreadable
            .note("")
            .code));
}

#[test]
fn invalid_tolerance_keeps_body_geometry_with_a_loss() {
    let mut partition = test_support::cube_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
        .unwrap()
        .unwrap();
    let range = records
        .nodes()
        .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::FACE)
        .unwrap()
        .field_range("tolerance")
        .unwrap();
    partition[range].copy_from_slice(&(-1.0f64).to_be_bytes());
    let bytes = test_support::with_partitions(&[partition]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().geometry.faces.len(), 6);
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::SolidEdgeLossCode::TopologyToleranceInvalid
            .note("")
            .code));
    assert_geometry_valid(result.ir());
}

fn assert_geometry_valid(ir: &cadmpeg_ir::CadIr) {
    let mut geometry = ir.clone();
    geometry.source = None;
    assert!(
        cadmpeg_ir::validate::validate_neutral(&geometry, Vec::new())
            .unwrap()
            .is_ok()
    );
}

#[test]
fn body_resolution_sets_document_linear_tolerance_in_millimetres() {
    let partitions: Vec<_> = [2.0e-8_f64, 4.0e-8]
        .into_iter()
        .map(|resolution| {
            let mut partition = test_support::sphere_partition();
            let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
                .unwrap()
                .unwrap();
            let range = records
                .nodes()
                .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::BODY)
                .unwrap()
                .field_range("res_linear")
                .unwrap();
            partition[range].copy_from_slice(&resolution.to_be_bytes());
            partition
        })
        .collect();
    let result = SolidEdgeCodec
        .decode(
            &mut Cursor::new(test_support::with_partitions(&partitions)),
            &DecodeOptions::default(),
        )
        .unwrap();
    let expected_mm = 4.0e-5;
    assert!((result.ir().tolerances.linear.get() - expected_mm).abs() < EPS_RESOLUTION_MM);
    assert_geometry_valid(result.ir());
}

#[test]
fn invalid_body_resolution_keeps_geometry_and_reports_a_loss() {
    let mut partition = test_support::sphere_partition();
    let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
        .unwrap()
        .unwrap();
    let range = records
        .nodes()
        .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::BODY)
        .unwrap()
        .field_range("res_linear")
        .unwrap();
    partition[range].copy_from_slice(&0.0f64.to_be_bytes());
    let result = SolidEdgeCodec
        .decode(
            &mut Cursor::new(test_support::with_partitions(&[partition])),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert_eq!(
        result.ir().tolerances.linear,
        cadmpeg_ir::CadIr::empty().tolerances.linear
    );
    assert!(result.report().losses.iter().any(|loss| loss.code
        == crate::loss::SolidEdgeLossCode::BodyResolutionInvalid
            .note("")
            .code));
    assert_geometry_valid(result.ir());
}

#[test]
fn empty_partition_preserves_stated_body_resolution() {
    let mut partition = test_support::sphere_partition();
    let (resolution, first_node) = {
        let records = cadmpeg_parasolid::records::PrimaryRecords::read(&partition)
            .unwrap()
            .unwrap();
        let body = records
            .nodes()
            .find(|node| node.kind() == cadmpeg_parasolid::records::node_type::BODY)
            .unwrap();
        (body.field_range("res_linear").unwrap(), body.range().start)
    };
    let mut empty = partition[..first_node].to_vec();
    empty.extend_from_slice(&[0, 1, 0, 1]);
    partition[resolution].copy_from_slice(&1.0e-12_f64.to_be_bytes());
    for partitions in [[partition.clone(), empty.clone()], [empty, partition]] {
        let result = SolidEdgeCodec
            .decode(
                &mut Cursor::new(test_support::with_partitions(&partitions)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(result.ir().geometry.bodies.len(), 1);
        assert!((result.ir().tolerances.linear.get() - 1.0e-9).abs() < EPS_RESOLUTION_MM);
        assert_geometry_valid(result.ir());
    }
}

#[test]
fn vertexless_ring_edge_is_bounded_by_one_vertex_on_its_carrier() {
    let bytes = test_support::with_partitions(&[test_support::disc_partition()]);
    let result = SolidEdgeCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let ir = result.ir();
    assert_eq!(ir.geometry.faces.len(), 1);
    assert_eq!(ir.geometry.edges.len(), 1);
    assert_eq!(ir.geometry.vertices.len(), 1);
    assert_eq!(ir.geometry.points.len(), 1);
    let ends = ir.geometry.edges[0].ends.as_ref().expect("bounded ring");
    assert_eq!(ends.start, ends.end);
    assert_eq!(ends.start, ir.geometry.vertices[0].id);
    let position = ir.geometry.points[0].position().get();
    let radius_mm = 5.0;
    let distance = (position.x * position.x + position.y * position.y).sqrt();
    assert!((distance - radius_mm).abs() < EPS_RING_VERTEX_MM);
    assert!(position.z.abs() < EPS_RING_VERTEX_MM);
    let range = ir.geometry.edges[0]
        .carrier
        .param_range()
        .expect("ring range")
        .get();
    assert!((range[1] - range[0] - std::f64::consts::TAU).abs() < EPS_RING_VERTEX_MM);
    assert_geometry_valid(ir);
}
