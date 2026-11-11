// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(super) fn operation_body_graph() -> SyntheticMeshGraph {
    let mut graph = synthetic_mesh_graph_with_body_count(false, 2);
    let collection_start = sole_typed_frame(&graph, MESH_COLLECTION_TYPE_GUID).start;
    graph.bytes[collection_start + mesh_collection::BODY_COUNT
        ..collection_start + mesh_collection::BODY_COUNT + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    let base_count = collection_start + mesh_collection::LEN + mesh_collection_base::BODY_COUNT;
    graph.bytes[base_count..base_count + 4].copy_from_slice(&1u32.to_le_bytes());
    let second_ref = collection_start + mesh_collection::LEN + mesh_collection_base::LEN + 11;
    graph.bytes.drain(second_ref..second_ref + 11);
    for record in &mut graph.meta.records {
        if record.bulk_offset > u64_from_index(second_ref) {
            record.bulk_offset -= 11;
        }
    }
    let scope_start = sole_typed_frame(&graph, MESH_FEATURE_SCOPE_TYPE_GUID).start;
    graph.bytes[scope_start + 21..scope_start + 25].copy_from_slice(&1u32.to_le_bytes());
    graph.bytes.drain(scope_start + 36..scope_start + 47);
    for record in &mut graph.meta.records {
        if record.bulk_offset > u64_from_index(scope_start + 36) {
            record.bulk_offset -= 11;
        }
    }
    let frames =
        typed_primary_frames(&graph.bytes, &graph.meta, MESH_BODY_TYPE_GUID, "test").unwrap();
    let body_start = frames
        .iter()
        .find(|frame| frame.entity_id == 117)
        .unwrap()
        .start;
    put_reference(
        &mut graph.bytes,
        body_start + mesh_body::FEATURE_SCOPE_REFERENCE,
        122,
    );
    graph.meta.types.push(design_type(
        super::super::MESH_CUT_SCOPE_TYPE_GUID,
        Some(super::super::MESH_CUT_SCOPE_BASE_TYPE_GUID),
        1,
        FUSION_MODULE,
        vec![122],
    ));
    graph.meta.types.push(design_type(
        super::super::MESH_OPERATION_GROUP_TYPE_GUID,
        Some(super::super::MESH_OPERATION_GROUP_BASE_TYPE_GUID),
        0,
        FUSION_MODULE,
        vec![123],
    ));
    graph.meta.types.push(design_type(
        super::super::MESH_OPERATION_MEMBER_TYPE_GUID,
        Some(super::super::MESH_OPERATION_MEMBER_BASE_TYPE_GUID),
        0,
        FUSION_MODULE,
        vec![126],
    ));
    let mut scope = mesh_scope_record(271, 264, 122, &[], 123, 112);
    let mut input_reference = Vec::new();
    push_reference(&mut input_reference, 103);
    scope.splice(25..25, input_reference);
    let mut original_kind = Vec::new();
    lp_utf16(&mut original_kind, "Base Mesh Feature");
    let mut new_kind = Vec::new();
    lp_utf16(&mut new_kind, "MeshPlaneCut");
    let kind_at = scope
        .windows(original_kind.len())
        .position(|window| window == original_kind)
        .unwrap();
    scope.splice(kind_at..kind_at + original_kind.len(), new_kind);
    push_primary_frame(&mut graph.bytes, &mut graph.meta.records, 122, scope);
    let mut group = Vec::new();
    push_indexed_header(&mut group, 272, 123);
    group.extend_from_slice(&[0; 10]);
    group.extend_from_slice(&1u32.to_le_bytes());
    push_reference(&mut group, 126);
    group.extend_from_slice(&[0; 2]);
    group.extend_from_slice(&1u32.to_le_bytes());
    push_reference(&mut group, 140);
    group.extend_from_slice(&0x0000_0008_0000_0000u64.to_le_bytes());
    group.extend_from_slice(&[0; 10]);
    group.extend_from_slice(&180u32.to_le_bytes());
    group.extend_from_slice(&0.125f64.to_le_bytes());
    group.extend_from_slice(&180u32.to_le_bytes());
    push_reference(&mut group, 125);
    group.extend_from_slice(&[1, 1, 0]);
    push_reference(&mut group, 124);
    group.push(0);
    push_reference(&mut group, 122);
    push_indexed_header(&mut group, 264, 123);
    push_primary_frame(&mut graph.bytes, &mut graph.meta.records, 123, group);
    let mut member = Vec::new();
    push_indexed_header(&mut member, 273, 126);
    member.extend_from_slice(&[0; 10]);
    push_reference(&mut member, 117);
    member.extend_from_slice(&[1, 2, 3, 4]);
    push_primary_frame(&mut graph.bytes, &mut graph.meta.records, 126, member);
    graph
}

#[test]
fn mesh_operation_member_links_an_unlisted_body() {
    let graph = operation_body_graph();
    let features = parse_mesh_design_records(
        &graph.bytes,
        &graph.meta,
        "Synthetic/BulkStream.dat",
        &mut no_texture_asset,
    )
    .unwrap();
    let [operation] = features[0].operation_bodies.as_slice() else {
        panic!("one operation body");
    };
    assert_eq!(
        (
            operation.scope.record.record_index(),
            operation.group.record.record_index(),
            operation.member.record.record_index()
        ),
        (122, 123, 126)
    );
    assert_eq!(operation.body.placement.record().record_index(), 117);
    assert_eq!(operation.body.scope_body_ordinal, None);
    let [input] = operation.inputs.as_slice() else {
        panic!("one operation input");
    };
    assert_eq!(input.guid.record.record_index(), 103);
    assert_eq!(input.entry.record.record_index(), 102);
    let wire = serde_json::to_value(&features[0]).unwrap();
    assert_eq!(
        wire["operation_bodies"][0]["member"]["record"]["record_index"],
        126
    );
    assert_eq!(
        wire["operation_bodies"][0]["inputs"][0]["guid"]["record"]["record_index"],
        103
    );
    assert_eq!(
        wire["operation_bodies"][0]["inputs"][0]["entry"]["record"]["record_index"],
        102
    );
    assert_eq!(
        serde_json::from_value::<crate::records::mesh::DesignMeshFeature>(wire).unwrap(),
        features[0]
    );
}

pub(super) fn shared_guid_graph() -> SyntheticMeshGraph {
    let mut graph = synthetic_mesh_graph_with_body_count(false, 2);
    let frames =
        typed_primary_frames(&graph.bytes, &graph.meta, MESH_BODY_TYPE_GUID, "test").unwrap();
    let second = frames
        .iter()
        .find(|frame| frame.entity_id == 117)
        .unwrap()
        .start;
    put_reference(
        &mut graph.bytes,
        second + mesh_body::CONTAINER_GUID_REFERENCE,
        103,
    );
    let mut removed = Vec::new();
    for guid in [MESH_ENTRY_NAME_TYPE_GUID, MESH_GUID_TYPE_GUID] {
        removed.extend(
            typed_primary_frames(&graph.bytes, &graph.meta, guid, "test")
                .unwrap()
                .into_iter()
                .filter(|frame| frame.entity_id == 115 || frame.entity_id == 116)
                .map(|frame| (frame.entity_id, frame.start, frame.end)),
        );
    }
    removed.sort_unstable_by_key(|(_, start, _)| std::cmp::Reverse(*start));
    for (entity, start, end) in removed {
        graph.bytes.drain(start..end);
        graph
            .meta
            .records
            .retain(|record| record.entity_id != entity);
        for record in &mut graph.meta.records {
            if record.bulk_offset > u64_from_index(start) {
                record.bulk_offset -= u64_from_index(end - start);
            }
        }
    }
    graph.meta.types[0] = design_type(
        MESH_ENTRY_NAME_TYPE_GUID,
        Some(MESH_ENTRY_NAME_BASE_TYPE_GUID),
        MESH_ENTRY_NAME_TYPE_VERSION,
        PARAMESH_MODULE,
        vec![102],
    );
    graph.meta.types[1] = design_type(
        MESH_GUID_TYPE_GUID,
        Some(MESH_GUID_BASE_TYPE_GUID),
        MESH_GUID_TYPE_VERSION,
        PARAMESH_MODULE,
        vec![103],
    );
    graph
}

#[test]
fn mesh_bodies_can_share_a_guid_and_container() {
    let graph = shared_guid_graph();
    let features = parse_mesh_design_records(
        &graph.bytes,
        &graph.meta,
        "Synthetic/BulkStream.dat",
        &mut no_texture_asset,
    )
    .unwrap();
    assert_eq!(
        features[0]
            .bodies()
            .iter()
            .map(|body| body.guid.record().record_index())
            .collect::<Vec<_>>(),
        [103, 103]
    );
    assert_eq!(
        resolve_mesh_bodies(
            &[features],
            "ParaMeshGeometry.11111111-2222-4333-8444-555555555555.paramesh",
            "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE"
        ),
        Some(vec![(0, 0, 0), (0, 0, 1)])
    );
}

pub(super) fn two_scope_graph() -> SyntheticMeshGraph {
    let mut graph = synthetic_mesh_graph_with_body_count(false, 2);
    let body_frames =
        typed_primary_frames(&graph.bytes, &graph.meta, MESH_BODY_TYPE_GUID, "test").unwrap();
    let second_body_start = body_frames
        .iter()
        .find(|frame| frame.entity_id == 117)
        .unwrap()
        .start;
    put_reference(
        &mut graph.bytes,
        second_body_start + mesh_body::FEATURE_SCOPE_REFERENCE,
        122,
    );
    let scope_start = sole_typed_frame(&graph, MESH_FEATURE_SCOPE_TYPE_GUID).start;
    graph.bytes[scope_start + 21..scope_start + 25].copy_from_slice(&1u32.to_le_bytes());
    graph.bytes.drain(scope_start + 36..scope_start + 47);
    for record in &mut graph.meta.records {
        if record.bulk_offset > u64_from_index(scope_start) {
            record.bulk_offset -= 11;
        }
    }
    graph.meta.types[7] = design_type(
        MESH_FEATURE_SCOPE_TYPE_GUID,
        Some(MESH_FEATURE_SCOPE_BASE_TYPE_GUID),
        MESH_FEATURE_SCOPE_TYPE_VERSION,
        FUSION_MODULE,
        vec![109, 122],
    );
    push_primary_frame(
        &mut graph.bytes,
        &mut graph.meta.records,
        122,
        mesh_scope_record(263, 264, 122, &[117], 140, 112),
    );
    graph
}

#[test]
fn mesh_collection_joins_bodies_from_two_scopes() {
    let graph = two_scope_graph();
    let features = parse_mesh_design_records(
        &graph.bytes,
        &graph.meta,
        "Synthetic/BulkStream.dat",
        &mut no_texture_asset,
    )
    .unwrap();
    let [feature] = features.as_slice() else {
        panic!("one collection");
    };
    assert_eq!(
        feature
            .scopes()
            .map(|scope| scope.record().record_index())
            .collect::<Vec<_>>(),
        [109, 122]
    );
    assert_eq!(
        feature
            .bodies()
            .iter()
            .map(|body| (
                body.placement.record().record_index(),
                body.scope_record_index,
                body.scope_body_ordinal
            ))
            .collect::<Vec<_>>(),
        [(104, 109, Some(0)), (117, 122, Some(0))]
    );
    let wire = serde_json::to_value(feature).unwrap();
    assert_eq!(wire["scopes"][1]["scope_record"]["record_index"], 122);
    assert_eq!(wire["bodies"][1]["scope_record_index"], 122);
    assert_eq!(wire["bodies"][1]["scope_body_ordinal"], 0);
    assert_eq!(
        serde_json::from_value::<crate::records::mesh::DesignMeshFeature>(wire).unwrap(),
        *feature
    );
}

#[test]
fn distinct_guid_records_for_one_container_do_not_join() {
    let graph = synthetic_mesh_graph(false);
    let parse = |stream| {
        parse_mesh_design_records(&graph.bytes, &graph.meta, stream, &mut no_texture_asset).unwrap()
    };
    let first = parse("First/BulkStream.dat");
    let second = parse("Second/BulkStream.dat");
    let entry = first[0].bodies()[0].entry.name().to_owned();
    let uuid = first[0].bodies()[0].guid.value().to_owned();
    assert_eq!(
        resolve_mesh_bodies(std::slice::from_ref(&first), &entry, &uuid),
        Some(vec![(0, 0, 0)])
    );
    assert_eq!(resolve_mesh_bodies(&[first, second], &entry, &uuid), None);
}
