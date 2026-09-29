// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::split_neutral_component_shells;

fn split_error(limit: u64, connected: bool, wire: bool, attached: bool) -> CodecError {
    let faces: &[u32] = if connected { &[1, 2] } else { &[1] };
    let adjacency = if connected {
        BTreeMap::from([(1, BTreeSet::from([2])), (2, BTreeSet::from([1]))])
    } else {
        BTreeMap::from([(1, BTreeSet::new())])
    };
    let face_vertices = BTreeMap::from([
        (1, BTreeSet::from([if attached { 1 } else { 3 }])),
        (2, BTreeSet::from([4])),
    ]);
    let wires = if wire {
        BTreeSet::from([10])
    } else {
        BTreeSet::new()
    };
    let edge_vertices = BTreeMap::from([(10, [1, 2])]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    split_neutral_component_shells(
        &ctx,
        faces,
        &wires,
        &adjacency,
        &face_vertices,
        &edge_vertices,
    )
    .expect_err("shell partition allocation refused")
}

fn assert_refusal(error: &CodecError, operation: &'static str) {
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn brep_remaining_face_nodes_refuse_collection_limit() {
    assert_refusal(
        &split_error(0, false, false, false),
        "creo B-rep remaining face nodes",
    );
}

#[test]
fn brep_shell_group_face_nodes_refuse_collection_limit() {
    assert_refusal(
        &split_error(1, false, false, false),
        "creo B-rep shell group face nodes",
    );
}

#[test]
fn brep_pending_shell_faces_refuse_collection_limit() {
    assert_refusal(
        &split_error(2, false, false, false),
        "creo B-rep pending shell faces",
    );
}

#[test]
fn brep_shell_face_ids_refuse_collection_limit() {
    assert_refusal(
        &split_error(3, false, false, false),
        "creo B-rep shell face IDs",
    );
}

#[test]
fn brep_shell_records_refuse_collection_limit() {
    assert_refusal(
        &split_error(4, false, false, false),
        "creo B-rep shell records",
    );
}

#[test]
fn brep_connected_shell_group_nodes_refuse_collection_limit() {
    assert_refusal(
        &split_error(4, true, false, false),
        "creo B-rep shell group face nodes",
    );
}

#[test]
fn brep_connected_pending_shell_faces_refuse_collection_limit() {
    assert_refusal(
        &split_error(5, true, false, false),
        "creo B-rep pending shell faces",
    );
}

#[test]
fn brep_attached_wire_nodes_refuse_collection_limit() {
    assert_refusal(
        &split_error(5, false, true, true),
        "creo B-rep attached wire nodes",
    );
}

#[test]
fn brep_unattached_wire_nodes_refuse_collection_limit() {
    assert_refusal(
        &split_error(5, false, true, false),
        "creo B-rep unattached wire nodes",
    );
}

#[test]
fn brep_unattached_wire_shell_record_refuses_collection_limit() {
    assert_refusal(
        &split_error(6, false, true, false),
        "creo B-rep shell records",
    );
}
