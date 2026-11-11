use super::*;

const STREAM: &str = "Design/BulkStream.dat";

fn invalid_mesh_features(graph: &SyntheticMeshGraph, omitted_scope: Option<u64>) -> Vec<String> {
    let features =
        parse_mesh_design_records(&graph.bytes, &graph.meta, STREAM, &mut no_texture_asset)
            .expect("mesh graph");
    let records = super::super::IndexedRecordOffsets::build(&graph.bytes);
    let mut scopes = Vec::new();
    for frame in typed_primary_frames(
        &graph.bytes,
        &graph.meta,
        MESH_FEATURE_SCOPE_TYPE_GUID,
        "test scope",
    )
    .unwrap()
    {
        if Some(frame.entity_id) == omitted_scope {
            continue;
        }
        let identity = super::super::record_identity(
            &graph.bytes[frame.start..frame.end],
            frame,
            "test scope",
        )
        .unwrap();
        let mut scope = super::super::parse_parameter_scope_with_type_guid(
            &graph.bytes,
            &records,
            identity.record_index(),
            identity.class_tag(),
            u64_from_index(frame.start),
            Some(MESH_FEATURE_SCOPE_TYPE_GUID),
        )
        .unwrap()
        .expect("mesh parameter scope");
        scope
            .try_edit(|draft| {
                draft.id = format!("f3d:{STREAM}:design-parameter-scope#{}", frame.start);
            })
            .unwrap();
        scopes.push(scope);
    }
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture");
    {
        let mut native = crate::test_support::native_test::f3d_native_mut(&mut ir);
        native.design_parameter_scopes.extend(scopes);
        native.design_mesh_features.extend(features);
    }
    crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &ir).expect("validate the native namespace")
    })
    .into_iter()
    .filter(|finding| {
        finding.message == "Fusion Design mesh feature has an invalid frame or object graph"
    })
    .filter_map(|finding| finding.entity)
    .collect()
}

#[test]
fn validation_accepts_collection_owners_of_versions_twenty_one_and_twenty_two() {
    assert!(invalid_mesh_features(&synthetic_mesh_graph(false), None).is_empty());
    assert!(
        invalid_mesh_features(&super::owner::version_twenty_one_owner_graph(), None).is_empty()
    );
}

#[test]
fn validation_accepts_a_collection_with_two_scopes() {
    let graph = super::collection::two_scope_graph();
    assert!(invalid_mesh_features(&graph, None).is_empty());
    assert_eq!(invalid_mesh_features(&graph, Some(122)).len(), 1);
}

#[test]
fn validation_accepts_bodies_that_share_a_guid_record() {
    assert!(invalid_mesh_features(&super::collection::shared_guid_graph(), None).is_empty());
}
