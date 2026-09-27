// SPDX-License-Identifier: Apache-2.0

use super::tests::{assert_codec_collection_refusal, assert_codec_retained_refusal, triangulated_face_archive};
use super::copy_shape_for_transfer;
use crate::brep::{TextEdgeRepresentation, TextTShape, TextTShapeGeometry};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

#[test]
fn topology_body_roots_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD topology body roots");
}

#[test]
fn shell_face_uses_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD shell face uses");
}

#[test]
fn face_connectivity_edge_keys_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD face connectivity edge keys");
}

#[test]
fn face_connectivity_vertex_keys_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD face connectivity vertex keys");
}

#[test]
fn face_connectivity_edge_identity_refuses_at_retained_limit() {
    assert_codec_retained_refusal(&triangulated_face_archive(), "FreeCAD face connectivity edge identity");
}

#[test]
fn face_connectivity_vertex_identity_refuses_at_retained_limit() {
    assert_codec_retained_refusal(&triangulated_face_archive(), "FreeCAD face connectivity vertex identity");
}

#[test]
fn wire_edge_uses_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD wire edge uses");
}

#[test]
fn loop_coedge_ids_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD loop coedge IDs");
}

#[test]
fn indexed_polygon_points_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD indexed polygon points");
}

#[test]
fn indexed_polygon_parameters_refuse_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD indexed polygon parameters");
}

#[test]
fn region_shape_copy_refuses_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD region shape copy");
}

#[test]
fn shell_shape_copy_refuses_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD shell shape copy");
}

#[test]
fn face_shape_copy_refuses_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD face shape copy");
}

#[test]
fn wire_shape_copy_refuses_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD wire shape copy");
}

#[test]
fn edge_shape_copy_refuses_at_collection_limit() {
    assert_codec_collection_refusal(&triangulated_face_archive(), "FreeCAD edge shape copy");
}

#[test]
fn edge_representation_continuity_refuses_at_retained_limit() {
    let shape = TextTShape {
        geometry: TextTShapeGeometry::Edge {
            tolerance: FiniteReal::ONE,
            same_parameter: false,
            same_range: false,
            degenerated: false,
            representations: vec![TextEdgeRepresentation::Regularity {
                continuity: "C1".to_owned(),
                surfaces: [1, 2],
                locations: [0, 0],
            }],
        },
        flags: [false; 7],
        children: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        copy_shape_for_transfer(&ctx, &shape, "FreeCAD edge shape copy"),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "FreeCAD edge shape copy"
    ));
}

#[test]
fn pcurve_pair_continuity_refuses_at_retained_limit() {
    let shape = TextTShape {
        geometry: TextTShapeGeometry::Edge {
            tolerance: FiniteReal::ONE,
            same_parameter: false,
            same_range: false,
            degenerated: false,
            representations: vec![TextEdgeRepresentation::PcurvePair {
                curves: [1, 2],
                continuity: "C1".to_owned(),
                surface: 1,
                location: 0,
                parameter_range: [FiniteReal::ZERO, FiniteReal::ONE],
                uv_endpoints: None,
            }],
        },
        flags: [false; 7],
        children: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        copy_shape_for_transfer(&ctx, &shape, "FreeCAD edge shape copy"),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "FreeCAD edge shape copy"
    ));
}

#[test]
fn topology_occurrence_identity_refuses_at_retained_limit() {
    assert_codec_retained_refusal(&triangulated_face_archive(), "FreeCAD topology occurrence identity");
}
