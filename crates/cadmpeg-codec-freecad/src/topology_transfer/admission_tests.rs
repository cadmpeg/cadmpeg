// SPDX-License-Identifier: Apache-2.0

use super::tests::{assert_codec_collection_refusal, assert_codec_retained_refusal, triangulated_face_archive};

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
