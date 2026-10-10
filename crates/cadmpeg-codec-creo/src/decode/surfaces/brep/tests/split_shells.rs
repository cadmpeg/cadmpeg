// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{split_neutral_component_shells, NeutralShellSpec};

fn split_error(operation: &'static str, connected: bool, wire: bool, attached: bool) -> CodecError {
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
    crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        operation,
        |ctx| {
            split_neutral_component_shells(
                ctx,
                faces,
                &wires,
                &adjacency,
                &face_vertices,
                &edge_vertices,
            )
        },
    )
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
fn shell_component_face_traversal_refuses_before_pop() {
    let shells = crate::test_support::assert_work_boundaries(
        &["creo B-rep shell component face visits"],
        |ctx| {
            split_neutral_component_shells(
                ctx,
                &[1],
                &BTreeSet::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
            )
        },
    );
    assert_eq!(
        shells,
        vec![NeutralShellSpec {
            faces: vec![1],
            wire_curves: BTreeSet::new(),
        }]
    );
}

#[test]
fn disconnected_shell_faces_admit_each_tree_removal() {
    let shells = crate::test_support::assert_work_boundaries(
        &["creo B-rep shell component face removal"],
        |ctx| {
            split_neutral_component_shells(
                ctx,
                &[1, 2, 3],
                &BTreeSet::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
            )
        },
    );
    assert_eq!(
        shells,
        [1, 2, 3].map(|face| NeutralShellSpec {
            faces: vec![face],
            wire_curves: BTreeSet::new(),
        })
    );
}

#[test]
fn brep_remaining_face_nodes_refuse_collection_limit() {
    assert_refusal(
        &split_error("creo B-rep remaining face nodes", false, false, false),
        "creo B-rep remaining face nodes",
    );
}

#[test]
fn brep_shell_group_face_nodes_refuse_collection_limit() {
    assert_refusal(
        &split_error("creo B-rep shell group face nodes", false, false, false),
        "creo B-rep shell group face nodes",
    );
}

#[test]
fn brep_pending_shell_faces_refuse_collection_limit() {
    assert_refusal(
        &split_error("creo B-rep pending shell faces", false, false, false),
        "creo B-rep pending shell faces",
    );
}

#[test]
fn brep_shell_face_ids_refuse_collection_limit() {
    assert_refusal(
        &split_error("creo B-rep shell face IDs", false, false, false),
        "creo B-rep shell face IDs",
    );
}

#[test]
fn brep_shell_records_refuse_collection_limit() {
    assert_refusal(
        &split_error("creo B-rep shell records", false, false, false),
        "creo B-rep shell records",
    );
}

#[test]
fn brep_connected_shell_group_nodes_refuse_collection_limit() {
    assert_refusal(
        &split_error("creo B-rep shell group face nodes", true, false, false),
        "creo B-rep shell group face nodes",
    );
}

#[test]
fn brep_connected_pending_shell_faces_refuse_collection_limit() {
    assert_refusal(
        &split_error("creo B-rep pending shell faces", true, false, false),
        "creo B-rep pending shell faces",
    );
}

#[test]
fn brep_attached_wire_nodes_refuse_collection_limit() {
    assert_refusal(
        &split_error("creo B-rep attached wire nodes", false, true, true),
        "creo B-rep attached wire nodes",
    );
}

#[test]
fn brep_unattached_wire_nodes_refuse_collection_limit() {
    assert_refusal(
        &split_error("creo B-rep unattached wire nodes", false, true, false),
        "creo B-rep unattached wire nodes",
    );
}

#[test]
fn brep_unattached_wire_shell_record_refuses_collection_limit() {
    assert_refusal(
        &split_error("creo B-rep shell records", false, true, false),
        "creo B-rep shell records",
    );
}

#[test]
fn pending_shell_face_traversal_refuses_before_pop() {
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        split_neutral_component_shells(
            &ctx,
            &[1],
            &BTreeSet::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
    };
    let limit = crate::test_support::allocation_limit_at(
        ResourceDimension::WorkUnits,
        Some("creo B-rep pending shell face traversal"),
        run,
    );
    assert!(
        matches!(run(limit), Err(CodecError::ResourceLimit(resource))
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo B-rep pending shell face traversal")
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| split_neutral_component_shells(
            ctx,
            &[1],
            &BTreeSet::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new()
        ))
        .expect("service shell partition"),
        vec![NeutralShellSpec {
            faces: vec![1],
            wire_curves: BTreeSet::new()
        }]
    );
}
