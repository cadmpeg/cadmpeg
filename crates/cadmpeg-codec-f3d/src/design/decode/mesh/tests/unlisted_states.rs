// SPDX-License-Identifier: Apache-2.0
//! Mesh geometry states and consumed bodies that no mesh collection lists.

use super::*;
use crate::design::decode::mesh_operations::{
    tessellate, MeshOperationKind, GEOMETRY_TAIL_SCOPE_BASE_TYPE_GUID,
};

const STATE_ENTRY: u32 = 130;
const STATE_GUID: u32 = 131;
const REDUCE_SCOPE: u32 = 132;
const PRODUCED_BODY: u32 = 141;
const PRODUCED_WRAPPER: u32 = 142;
const PRODUCED_GUID: u32 = 143;
const PRODUCED_ENTRY: u32 = 144;
const TESSELLATE_SCOPE: u32 = 145;
const OWNER_PLACEHOLDER: u32 = 150;
const GROUP_PLACEHOLDER: u32 = 151;

fn parse(
    graph: &SyntheticMeshGraph,
) -> Result<Vec<crate::records::mesh::DesignMeshFeature>, CodecError> {
    parse_mesh_design_records(
        &graph.bytes,
        &graph.meta,
        "Synthetic/BulkStream.dat",
        &mut no_texture_asset,
    )
}

/// Register `entities` as the only records of the type at `ordinal` of the
/// synthetic graph.
fn register(graph: &mut SyntheticMeshGraph, ordinal: usize, entities: Vec<u64>) {
    graph.meta.types[ordinal].entities =
        crate::records::identity::ReferenceRun::unlocated(entities);
}

/// Append a type row and return its class tag.
fn push_type(
    graph: &mut SyntheticMeshGraph,
    type_guid: &str,
    base_type_guid: &str,
    version: u32,
    entity: u32,
) -> u32 {
    graph.meta.types.push(design_type(
        type_guid,
        Some(base_type_guid),
        version,
        FUSION_MODULE,
        vec![u64::from(entity)],
    ));
    256 + u32::try_from(graph.meta.types.len() - 1).unwrap()
}

/// Append an entry-name and GUID pair that no mesh body names.
fn push_state_pair(graph: &mut SyntheticMeshGraph, entry: u32, guid: u32, name: &str, uuid: &str) {
    push_primary_frame(
        &mut graph.bytes,
        &mut graph.meta.records,
        entry,
        mesh_entry_record(256, entry, guid, name),
    );
    push_primary_frame(
        &mut graph.bytes,
        &mut graph.meta.records,
        guid,
        mesh_guid_record(257, guid, entry, uuid),
    );
}

/// The one-body graph with an entry-name and GUID pair that no body names.
/// With `reduce`, a `MeshReduce` scope names the GUID record as the input
/// geometry state of its geometry tail.
fn geometry_state_graph(reduce: bool) -> SyntheticMeshGraph {
    let mut graph = synthetic_mesh_graph(false);
    register(&mut graph, 0, vec![102, u64::from(STATE_ENTRY)]);
    register(&mut graph, 1, vec![103, u64::from(STATE_GUID)]);
    push_state_pair(
        &mut graph,
        STATE_ENTRY,
        STATE_GUID,
        "ParaMeshGeometry.CCCCCCCC-DDDD-4EEE-8FFF-000000000000.paramesh",
        "DDDDDDDD-EEEE-4FFF-8000-111111111111",
    );
    if reduce {
        let (type_guid, version) = MeshOperationKind::Reduce.registration();
        let tag = push_type(
            &mut graph,
            type_guid,
            GEOMETRY_TAIL_SCOPE_BASE_TYPE_GUID,
            version,
            REDUCE_SCOPE,
        );
        let mut scope = Vec::new();
        push_indexed_header(&mut scope, tag, REDUCE_SCOPE);
        scope.extend_from_slice(&[0; 10]);
        push_reference(&mut scope, OWNER_PLACEHOLDER);
        scope.extend_from_slice(&1u32.to_le_bytes());
        scope.push(0);
        push_reference(&mut scope, OWNER_PLACEHOLDER);
        scope.extend_from_slice(&1u32.to_le_bytes());
        push_reference(&mut scope, OWNER_PLACEHOLDER);
        push_reference(&mut scope, 103);
        push_reference(&mut scope, STATE_GUID);
        scope.extend_from_slice(&0u32.to_le_bytes());
        push_reference(&mut scope, GROUP_PLACEHOLDER);
        scope.push(0);
        push_primary_frame(
            &mut graph.bytes,
            &mut graph.meta.records,
            REDUCE_SCOPE,
            scope,
        );
    }
    graph
}

#[test]
fn a_geometry_state_that_a_mesh_operation_names_joins_no_body() {
    let features = parse(&geometry_state_graph(true)).expect("mesh graph");
    let [feature] = features.as_slice() else {
        panic!("one mesh feature");
    };
    assert_eq!(
        feature
            .all_bodies()
            .map(|body| body.guid.record().record_index())
            .collect::<Vec<_>>(),
        [103]
    );
    assert!(
        parse(&geometry_state_graph(false)).is_err(),
        "a GUID record that no body and no mesh operation names is a leftover"
    );
}

/// The one-body graph with a second body that names the collection, which
/// does not list it. With `produced`, the second body names a
/// `MeshTessellate` scope in its feature-scope slot.
fn consumed_body_graph(produced: bool) -> SyntheticMeshGraph {
    let mut graph = synthetic_mesh_graph(false);
    register(&mut graph, 0, vec![102, u64::from(PRODUCED_ENTRY)]);
    register(&mut graph, 1, vec![103, u64::from(PRODUCED_GUID)]);
    register(&mut graph, 2, vec![104, u64::from(PRODUCED_BODY)]);
    register(&mut graph, 6, vec![108, u64::from(PRODUCED_WRAPPER)]);
    push_state_pair(
        &mut graph,
        PRODUCED_ENTRY,
        PRODUCED_GUID,
        "ParaMeshGeometry.EEEEEEEE-FFFF-4000-8111-222222222222.paramesh",
        "FFFFFFFF-0000-4111-8222-333333333333",
    );
    push_primary_frame(
        &mut graph.bytes,
        &mut graph.meta.records,
        PRODUCED_WRAPPER,
        mesh_wrapper_record(262, PRODUCED_WRAPPER, PRODUCED_BODY),
    );
    push_primary_frame(
        &mut graph.bytes,
        &mut graph.meta.records,
        PRODUCED_BODY,
        mesh_body_record(
            258,
            PRODUCED_BODY,
            PRODUCED_GUID,
            TESSELLATE_SCOPE,
            PRODUCED_WRAPPER,
            111,
            107,
            100,
        ),
    );
    if produced {
        let tag = push_type(
            &mut graph,
            tessellate::SCOPE_TYPE_GUID,
            tessellate::SCOPE_BASE_TYPE_GUID,
            tessellate::SCOPE_TYPE_VERSION,
            TESSELLATE_SCOPE,
        );
        let mut scope = Vec::new();
        push_indexed_header(&mut scope, tag, TESSELLATE_SCOPE);
        scope.extend_from_slice(&[0; 10]);
        push_primary_frame(
            &mut graph.bytes,
            &mut graph.meta.records,
            TESSELLATE_SCOPE,
            scope,
        );
    }
    graph
}

#[test]
fn a_consumed_produced_body_that_no_collection_lists_joins_no_feature() {
    let features = parse(&consumed_body_graph(true)).expect("mesh graph");
    let [feature] = features.as_slice() else {
        panic!("one mesh feature");
    };
    assert_eq!(
        feature
            .all_bodies()
            .map(|body| body.placement.record().record_index())
            .collect::<Vec<_>>(),
        [104]
    );
    assert!(
        parse(&consumed_body_graph(false)).is_err(),
        "an unlisted body whose feature scope is no MeshTessellate scope is a leftover"
    );
}
