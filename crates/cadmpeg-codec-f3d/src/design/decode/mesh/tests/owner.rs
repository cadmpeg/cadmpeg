use super::*;

#[test]
fn mesh_graph_accepts_alternate_body_owner_registration() {
    let mut graph = synthetic_mesh_graph(false);
    let owner = graph
        .meta
        .types
        .iter_mut()
        .find(|ty| {
            ty.type_guid
                .as_str()
                .eq_ignore_ascii_case(MESH_BODY_OWNER_TYPE_GUID)
        })
        .unwrap();
    *owner = design_type(
        super::super::MESH_BODY_OWNER_ALTERNATE_TYPE_GUID,
        Some(super::super::MESH_BODY_OWNER_ALTERNATE_BASE_TYPE_GUID),
        super::super::MESH_BODY_OWNER_ALTERNATE_TYPE_VERSION,
        "Body",
        vec![111],
    );
    let features = parse_mesh_design_records(
        &graph.bytes,
        &graph.meta,
        "Synthetic/BulkStream.dat",
        &mut no_texture_asset,
    )
    .expect("alternate Body owner");
    assert_eq!(features.len(), 1);
    assert_eq!(features[0].bodies()[0].owner_record.record_index(), 111);
}

#[test]
fn named_collection_owner_keeps_its_frame() {
    for version in [19, 20] {
        let mut graph = synthetic_mesh_graph(false);
        graph
            .meta
            .types
            .iter_mut()
            .find(|ty| {
                ty.type_guid
                    .as_str()
                    .eq_ignore_ascii_case(MESH_COLLECTION_OWNER_TYPE_GUID)
            })
            .unwrap()
            .version = version;
        let frame = sole_typed_frame(&graph, MESH_COLLECTION_OWNER_TYPE_GUID);
        let start = frame.start;
        let old_end = frame.end;
        let mut owner = Vec::new();
        push_indexed_header(&mut owner, 268, 110);
        owner.extend_from_slice(&[0; 10]);
        owner.extend_from_slice(&2_u32.to_le_bytes());
        lp_utf16(&mut owner, "arrangementsContainer");
        push_reference(&mut owner, 111);
        lp_utf16(&mut owner, "netfabb.mesh_body");
        push_reference(&mut owner, 100);
        owner.extend_from_slice(&[0; 16]);
        let expected = owner.clone();
        let delta = i64::try_from(owner.len()).unwrap() - i64::try_from(old_end - start).unwrap();
        graph.bytes.splice(start..old_end, owner);
        for record in &mut graph.meta.records {
            if record.bulk_offset > u64_from_index(start) {
                record.bulk_offset = record.bulk_offset.checked_add_signed(delta).unwrap();
            }
        }
        let features = parse_mesh_design_records(
            &graph.bytes,
            &graph.meta,
            "Synthetic/BulkStream.dat",
            &mut no_texture_asset,
        )
        .unwrap();
        let [feature] = features.as_slice() else {
            panic!("one feature")
        };
        let wire = serde_json::to_value(feature).unwrap();
        let decoded: crate::records::mesh::DesignMeshFeature =
            serde_json::from_value(wire).unwrap();
        assert_eq!(
            decoded.collection_owner.native_frame().unwrap().as_ref(),
            expected.as_slice()
        );
        assert_eq!(decoded, *feature);
        let losses: Vec<_> = super::super::mesh_owner_frame_losses(&features).collect();
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "mesh.owner-bytes-uninterpreted"
        );
    }
}

#[test]
fn mesh_collection_owner_accepts_legacy_version_fifteen() {
    const EXPECTED_COLLECTION: u32 = 100;
    let mut graph = synthetic_mesh_graph(false);
    graph
        .meta
        .types
        .iter_mut()
        .find(|design_type| {
            design_type
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(MESH_COLLECTION_OWNER_TYPE_GUID)
        })
        .expect("collection-owner type")
        .version = MESH_COLLECTION_OWNER_TYPE_VERSIONS[0];
    let design_type = graph
        .meta
        .types
        .iter()
        .find(|design_type| {
            design_type
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(MESH_COLLECTION_OWNER_TYPE_GUID)
        })
        .expect("collection-owner type");
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, 270, 110);
    bytes.resize(859, 0);
    push_reference(&mut bytes, EXPECTED_COLLECTION);
    let frame = TypedPrimaryFrame {
        entity_id: 110,
        start: 0,
        end: bytes.len(),
        design_type,
    };

    let owner = parse_mesh_collection_owner_record(&bytes, frame)
        .expect("valid legacy owner frame")
        .expect("legacy collection owner");
    assert_eq!(owner.collection_record_index, EXPECTED_COLLECTION);
    assert_eq!(owner.owner.backlink_offset(), 859);
}

#[test]
fn mesh_collection_owner_accepts_version_seventeen_frame() {
    const EXPECTED_COLLECTION: u32 = 100;
    let mut graph = synthetic_mesh_graph(false);
    let design_type = graph
        .meta
        .types
        .iter_mut()
        .find(|design_type| {
            design_type
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(MESH_COLLECTION_OWNER_TYPE_GUID)
        })
        .expect("collection-owner type");
    design_type.version = MESH_COLLECTION_OWNER_TYPE_VERSIONS[1];
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, 270, 110);
    bytes.resize(collection_owner_v17::COLLECTION_BACKLINK, 0);
    push_reference(&mut bytes, EXPECTED_COLLECTION);
    bytes.extend_from_slice(&[0; 64]);
    let frame = TypedPrimaryFrame {
        entity_id: 110,
        start: 0,
        end: bytes.len(),
        design_type,
    };

    let owner = parse_mesh_collection_owner_record(&bytes, frame)
        .expect("valid version-17 owner frame")
        .expect("version-17 collection owner");
    assert_eq!(owner.collection_record_index, EXPECTED_COLLECTION);
    assert_eq!(
        owner.owner.backlink_offset(),
        u64_from_index(collection_owner_v17::COLLECTION_BACKLINK)
    );
}

fn owner_frame_bytes(name: Option<&str>, backlink_at: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, 270, 110);
    bytes.resize(collection_owner_v21::NAME_COUNT, 0);
    if let Some(name) = name {
        let units: Vec<u16> = name.encode_utf16().collect();
        bytes.extend_from_slice(&u32::try_from(units.len()).unwrap().to_le_bytes());
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    }
    bytes.resize(backlink_at, 0);
    push_reference(&mut bytes, 100);
    bytes.extend_from_slice(&[0; 32]);
    bytes
}

#[test]
fn mesh_collection_owner_versions_twenty_one_and_twenty_two_locate_their_backlinks() {
    let mut graph = synthetic_mesh_graph(false);
    for (version, name, backlink_at, name_count) in [
        (21, Some("1_3"), 272, Some(3)),
        (21, Some(""), 266, Some(0)),
        (22, None, 262, None),
    ] {
        let design_type = graph
            .meta
            .types
            .iter_mut()
            .find(|design_type| {
                design_type
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case(MESH_COLLECTION_OWNER_TYPE_GUID)
            })
            .expect("collection-owner type");
        design_type.version = version;
        let bytes = owner_frame_bytes(name, backlink_at);
        let frame = TypedPrimaryFrame {
            entity_id: 110,
            start: 0,
            end: bytes.len(),
            design_type,
        };
        let owner = parse_mesh_collection_owner_record(&bytes, frame)
            .expect("valid owner frame")
            .expect("collection owner");
        assert_eq!(owner.collection_record_index, 100);
        assert_eq!(owner.owner.backlink_offset(), u64_from_index(backlink_at));
        assert_eq!(owner.owner.name_count(), name_count);
    }
}

pub(super) fn version_twenty_one_owner_graph() -> SyntheticMeshGraph {
    let mut graph = synthetic_mesh_graph(false);
    graph
        .meta
        .types
        .iter_mut()
        .find(|design_type| {
            design_type
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(MESH_COLLECTION_OWNER_TYPE_GUID)
        })
        .expect("collection-owner type")
        .version = 21;
    let start = sole_typed_frame(&graph, MESH_COLLECTION_OWNER_TYPE_GUID).start;
    let name = [3, 0, 0, 0, b'1', 0, b'_', 0, b'3', 0];
    graph.bytes.splice(start + 25..start + 25, name);
    for record in &mut graph.meta.records {
        if record.bulk_offset > u64::try_from(start).unwrap() {
            record.bulk_offset += 10;
        }
    }
    graph
}

#[test]
fn mesh_graph_joins_a_version_twenty_one_collection_owner() {
    let graph = version_twenty_one_owner_graph();
    let start = sole_typed_frame(&graph, MESH_COLLECTION_OWNER_TYPE_GUID).start;
    let features = parse_mesh_design_records(
        &graph.bytes,
        &graph.meta,
        "Synthetic/BulkStream.dat",
        &mut no_texture_asset,
    )
    .expect("mesh graph with a version-21 owner");
    let [feature] = features.as_slice() else {
        panic!("one mesh feature");
    };
    let wire = serde_json::to_value(feature).unwrap();
    assert_eq!(wire["collection_owner_name_count"], 3);
    assert_eq!(wire["collection_owner_backlink_offset"], start + 272);
    let decoded: crate::records::mesh::DesignMeshFeature = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded, *feature);
}
