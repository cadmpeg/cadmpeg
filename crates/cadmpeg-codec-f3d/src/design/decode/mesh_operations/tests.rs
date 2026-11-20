use super::*;

fn reference(target: u32) -> [u8; 11] {
    let mut bytes = [0; 11];
    bytes[0] = 1;
    bytes[1..5].copy_from_slice(&target.to_le_bytes());
    bytes
}

fn record(index: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    bytes[..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"714");
    bytes[7..11].copy_from_slice(&index.to_le_bytes());
    bytes
}

fn put(bytes: &mut [u8], offset: usize, value: &[u8]) {
    bytes[offset..offset + value.len()].copy_from_slice(value);
}

fn tail(bytes: &mut [u8], offset: usize, output: u32, input: u32, group: u32) {
    put(bytes, offset, &reference(output));
    put(bytes, offset + 11, &reference(input));
    put(bytes, offset + 26, &reference(group));
}

fn flagged(scope: &[u8], word: u32) -> Vec<u8> {
    let mut bytes = scope[..20].to_vec();
    bytes.push(1);
    bytes.extend_from_slice(&word.to_le_bytes());
    bytes.extend_from_slice(&scope[21..]);
    bytes
}

#[test]
fn removal_scope_reads_its_source_before_the_counted_scope_guid() {
    let mut scope = record(7, 108);
    put(&mut scope, 21, &reference(8));
    put(&mut scope, 32, &36u32.to_le_bytes());
    let read = read_scope(MeshOperationKind::Remove, &scope).unwrap();
    assert_eq!(read.fields, MeshOperationFields::Remove);
    assert_eq!(read.source_group, 8);
    assert_eq!(read.input_geometry, 0);
    assert_eq!(read.output_geometry, None);
    assert_eq!(read.tail_error, None);
    assert!(read.deviations.is_empty());
    let read = read_scope(MeshOperationKind::Remove, &flagged(&scope, 9)).unwrap();
    assert_eq!(read.source_group, 8);
    assert_eq!(read.prefix_word, Some(9));
    assert_eq!(read.tail_error, None);
}

#[test]
fn removal_scope_keeps_its_kind_when_the_source_reference_is_truncated() {
    let mut scope = record(7, 31);
    put(&mut scope, 21, &[1, 8, 0, 0, 0]);
    let read = read_scope(MeshOperationKind::Remove, &scope).unwrap();
    assert_eq!(read.fields, MeshOperationFields::Remove);
    assert_eq!(read.source_group, 0);
    assert_eq!(
        read.tail_error,
        Some(MeshOperationFraming("source group reference"))
    );
}

#[test]
fn reverse_normals_scope_stores_its_geometry_tail_at_offset_21() {
    let mut scope = record(7, 128);
    tail(&mut scope, 21, 30, 20, 8);
    let read = read_scope(MeshOperationKind::ReverseNormals, &scope).unwrap();
    assert_eq!(read.fields, MeshOperationFields::ReverseNormals);
    assert_eq!(read.prefix_word, None);
    assert_eq!(read.output_geometry, Some(30));
    assert_eq!(read.input_geometry, 20);
    assert_eq!(read.source_group, 8);
    assert!(read.deviations.is_empty());
}

#[test]
fn flagged_scope_stores_a_prefix_word_and_moves_its_fields_four_bytes() {
    let mut scope = record(7, 128);
    put(&mut scope, 21, &reference(11));
    tail(&mut scope, 32, 30, 20, 8);
    let read = read_scope(MeshOperationKind::Shell, &flagged(&scope, 9)).unwrap();
    assert_eq!(
        read.fields,
        MeshOperationFields::Shell {
            thickness_owner: 11
        }
    );
    assert_eq!(read.prefix_word, Some(9));
    assert_eq!(read.source_group, 8);
}

#[test]
fn smooth_scope_stores_its_owner_at_offset_21() {
    let mut scope = record(7, 128);
    put(&mut scope, 21, &reference(11));
    tail(&mut scope, 32, 30, 20, 8);
    let read = read_scope(MeshOperationKind::Smooth, &scope).unwrap();
    assert_eq!(
        read.fields,
        MeshOperationFields::Smooth {
            smoothness_owner: 11
        }
    );
    assert_eq!(read.input_geometry, 20);
}

#[test]
fn reduce_scope_stores_three_owners_and_two_selectors() {
    let mut scope = record(7, 128);
    put(&mut scope, 21, &reference(12));
    put(&mut scope, 32, &1u32.to_le_bytes());
    put(&mut scope, 37, &reference(11));
    put(&mut scope, 48, &2u32.to_le_bytes());
    put(&mut scope, 52, &reference(13));
    tail(&mut scope, 63, 30, 20, 8);
    let read = read_scope(MeshOperationKind::Reduce, &scope).unwrap();
    assert_eq!(
        read.fields,
        MeshOperationFields::Reduce {
            tolerance_owner: 12,
            distribution: 1,
            proportion_owner: 11,
            target: 2,
            face_count_owner: 13,
        }
    );
    assert_eq!(read.output_geometry, Some(30));
    assert_eq!(read.source_group, 8);
}

#[test]
fn repair_scope_stores_two_owners_and_two_selectors() {
    let mut scope = record(7, 128);
    put(&mut scope, 21, &reference(11));
    put(&mut scope, 32, &reference(12));
    put(&mut scope, 43, &2u32.to_le_bytes());
    put(&mut scope, 47, &3u32.to_le_bytes());
    tail(&mut scope, 51, 30, 20, 8);
    let read = read_scope(MeshOperationKind::Repair, &scope).unwrap();
    assert_eq!(
        read.fields,
        MeshOperationFields::Repair {
            density_owner: 11,
            offset_owner: 12,
            rebuild_mode: 2,
            method: 3,
        }
    );
    assert_eq!(read.input_geometry, 20);
}

#[test]
fn remesh_scope_stores_its_shape_preservation_owner_before_the_edge_length() {
    let mut scope = record(7, 128);
    put(&mut scope, 27, &reference(11));
    put(&mut scope, 39, &reference(12));
    put(&mut scope, 50, &1u32.to_le_bytes());
    tail(&mut scope, 54, 30, 20, 8);
    let read = read_scope(MeshOperationKind::Remesh, &scope).unwrap();
    assert_eq!(
        read.fields,
        MeshOperationFields::Remesh {
            distribution: 0,
            shape_preservation_owner: Some(11),
            edge_length_owner: 12,
        }
    );
    assert!(read.deviations.is_empty());
    assert_eq!(read.source_group, 8);
}

#[test]
fn uniform_remesh_scope_stores_a_null_shape_preservation_reference() {
    let mut scope = record(7, 128);
    put(&mut scope, 23, &1u32.to_le_bytes());
    put(&mut scope, 29, &reference(12));
    put(&mut scope, 40, &2u32.to_le_bytes());
    tail(&mut scope, 44, 30, 20, 8);
    let read = read_scope(MeshOperationKind::Remesh, &scope).unwrap();
    assert_eq!(
        read.fields,
        MeshOperationFields::Remesh {
            distribution: 1,
            shape_preservation_owner: None,
            edge_length_owner: 12,
        }
    );
    assert_eq!(read.deviations, [("remesh-word-after-edge-length", 2)]);
}

#[test]
fn section_scope_stores_a_null_output_geometry_reference() {
    let mut scope = record(7, 128);
    put(&mut scope, 21, &reference(8));
    put(&mut scope, 32, &reference(5));
    put(&mut scope, 44, &reference(20));
    put(&mut scope, 59, &reference(8));
    let read = read_scope(MeshOperationKind::Section, &scope).unwrap();
    assert_eq!(
        read.fields,
        MeshOperationFields::Section {
            section_group: 8,
            sketch_scope: 5,
        }
    );
    assert_eq!(read.output_geometry, None);
    assert_eq!(read.input_geometry, 20);
    assert_eq!(read.source_group, 8);
}

#[test]
fn scope_framing_failures_name_the_field() {
    let mut scope = record(7, 128);
    tail(&mut scope, 21, 30, 20, 8);
    let mut nonzero = scope.clone();
    nonzero[15] = 1;
    assert_eq!(
        read_scope(MeshOperationKind::ReverseNormals, &nonzero),
        Err(MeshOperationFraming("scope zero run"))
    );
    let mut flag = scope.clone();
    flag[20] = 2;
    assert_eq!(
        read_scope(MeshOperationKind::ReverseNormals, &flag),
        Err(MeshOperationFraming("scope flag"))
    );
    assert_eq!(
        read_scope(MeshOperationKind::ReverseNormals, &scope[..40])
            .unwrap()
            .tail_error,
        Some(MeshOperationFraming("input geometry reference"))
    );
    let mut null_input = scope;
    put(&mut null_input, 32, &[0; 11]);
    assert_eq!(
        read_scope(MeshOperationKind::ReverseNormals, &null_input)
            .unwrap()
            .tail_error,
        Some(MeshOperationFraming("input geometry reference"))
    );
}

fn source_stream(
    count: u32,
    members: &[(u32, u32)],
) -> (Vec<u8>, std::sync::Arc<crate::metastream::MetaStream>) {
    use crate::metastream::{MetaStream, RecordIndexEntry};
    use crate::records::entity_header::{BaseTypeGuid, SegmentTypeData};
    use crate::records::identity::ReferenceRun;

    let mut stream = record(8, 25 + 11 * members.len());
    stream[4..7].copy_from_slice(b"256");
    let mut records = vec![RecordIndexEntry {
        entity_id: 8,
        bulk_offset: 0,
    }];
    put(&mut stream, 21, &count.to_le_bytes());
    for (ordinal, (member, _)) in members.iter().enumerate() {
        put(&mut stream, 25 + 11 * ordinal, &reference(*member));
    }
    for (member, body) in members {
        let mut bytes = record(*member, 40);
        bytes[4..7].copy_from_slice(b"257");
        put(&mut bytes, 21, &reference(*body));
        records.push(RecordIndexEntry {
            entity_id: u64::from(*member),
            bulk_offset: u64::try_from(stream.len()).unwrap(),
        });
        stream.extend_from_slice(&bytes);
    }
    for (_, body) in members {
        let mut bytes = record(*body, 11);
        bytes[4..7].copy_from_slice(b"258");
        records.push(RecordIndexEntry {
            entity_id: u64::from(*body),
            bulk_offset: u64::try_from(stream.len()).unwrap(),
        });
        stream.extend_from_slice(&bytes);
    }
    let row = |guid: &str, base: &str, entities: Vec<u64>| SegmentTypeData {
        byte_offset: 0,
        type_guid: guid.to_owned().try_into().unwrap(),
        type_guid_offset: 0,
        base_type_guid: BaseTypeGuid::Guid {
            value: base.to_owned().try_into().unwrap(),
            offset: 0,
        },
        version: 0,
        version_offset: 0,
        module: "Fusion".into(),
        entities: ReferenceRun::unlocated(entities),
    };
    let mut body_row = row(
        "EA90DA22-556C-4C61-89BB-20C2681B7A9D",
        "CB844AB6-240D-4FC9-9C9F-3679DC896D6F",
        members.iter().map(|(_, body)| u64::from(*body)).collect(),
    );
    body_row.module = "ParaMesh".into();
    let meta = MetaStream {
        types: vec![
            row(
                "22975A91-16AD-4CC9-A5E4-B4F8B4C27881",
                "2CA5A1CD-C99B-4C9B-91AF-57989148E841",
                vec![8],
            ),
            row(
                "1AA9CC7B-5C34-45DA-AE14-0444E7F966F2",
                "6144EA59-3ACA-4172-9F14-A6531E94D69B",
                members.iter().map(|(index, _)| u64::from(*index)).collect(),
            ),
            body_row,
        ],
        records,
        secondary_records: Vec::new(),
    };
    (stream, std::sync::Arc::new(meta))
}

#[test]
fn source_group_members_name_their_bodies_in_group_order() {
    let (stream, meta) = source_stream(2, &[(41, 9), (40, 3)]);
    let bodies = crate::test_support::with_decode_context(|ctx| {
        MeshOperationRecords::index(View::over_retained(&stream), meta).group_bodies(ctx, 8)
    })
    .unwrap();
    assert_eq!(bodies, Ok(vec![9, 3]));
}

#[test]
fn null_source_group_reference_ignores_zero_identity_metadata() {
    let (mut stream, mut meta) = source_stream(1, &[(41, 9)]);
    put(&mut stream, 7, &0u32.to_le_bytes());
    let changed = std::sync::Arc::make_mut(&mut meta);
    changed.records[0].entity_id = 0;
    changed.types[0].entities = crate::records::identity::ReferenceRun::unlocated(vec![0]);
    let bodies = crate::test_support::with_decode_context(|ctx| {
        MeshOperationRecords::index(View::over_retained(&stream), meta).group_bodies(ctx, 0)
    })
    .unwrap();
    assert_eq!(bodies, Err(MeshOperationFraming("source group record")));
}

#[test]
fn source_group_member_count_is_bounded_before_allocation() {
    for (count, members) in [(u32::MAX, vec![(41, 9)]), (0, vec![])] {
        let (stream, meta) = source_stream(count, &members);
        let bodies = crate::test_support::with_decode_context(|ctx| {
            MeshOperationRecords::index(View::over_retained(&stream), meta).group_bodies(ctx, 8)
        })
        .unwrap();
        assert_eq!(
            bodies,
            Err(MeshOperationFraming("source group member count"))
        );
    }
}

#[test]
fn missing_source_records_name_the_unresolved_hop() {
    let (stream, meta) = source_stream(1, &[(41, 9)]);
    let records = MeshOperationRecords::index(View::over_retained(&stream[..36]), meta);
    let bodies = crate::test_support::with_decode_context(|ctx| {
        Ok::<_, CodecError>((
            records.group_bodies(ctx, 8)?,
            records.group_bodies(ctx, 77)?,
        ))
    })
    .unwrap();
    assert_eq!(
        bodies,
        (
            Err(MeshOperationFraming("source member record")),
            Err(MeshOperationFraming("source group record"))
        )
    );
}

#[test]
fn selected_records_require_the_registered_type_and_unique_index() {
    let (stream, meta) = source_stream(1, &[(41, 9)]);
    for change in 0..4 {
        let mut changed = meta.clone();
        let row = std::sync::Arc::make_mut(&mut changed);
        match change {
            0 => row.types[1].version = 1,
            1 => row.types[1].module = "Other".into(),
            2 => {
                row.types[1].entities = crate::records::identity::ReferenceRun::unlocated(vec![42]);
            }
            _ => row.records.push(row.records[1]),
        }
        let result = crate::test_support::with_decode_context(|ctx| {
            MeshOperationRecords::index(View::over_retained(&stream), changed).group_bodies(ctx, 8)
        })
        .unwrap();
        assert_eq!(result, Err(MeshOperationFraming("source member record")));
    }
}

#[test]
fn source_body_identity_is_validated_without_reading_its_payload_version() {
    let (stream, mut meta) = source_stream(1, &[(41, 9)]);
    std::sync::Arc::make_mut(&mut meta).types[2].version = 999;
    let read = crate::test_support::with_decode_context(|ctx| {
        MeshOperationRecords::index(View::over_retained(&stream), meta.clone()).group_bodies(ctx, 8)
    })
    .unwrap();
    assert_eq!(read, Ok(vec![9]));
    std::sync::Arc::make_mut(&mut meta).types[2].module = "Other".into();
    let read = crate::test_support::with_decode_context(|ctx| {
        MeshOperationRecords::index(View::over_retained(&stream), meta).group_bodies(ctx, 8)
    })
    .unwrap();
    assert_eq!(read, Err(MeshOperationFraming("source body record")));
}

#[test]
fn member_reference_cannot_cross_its_secondary_boundary() {
    let (mut stream, mut meta) = source_stream(1, &[(41, 9)]);
    let mut nested = record(41, 11);
    nested[4..7].copy_from_slice(b"257");
    put(&mut stream, 61, &nested);
    std::sync::Arc::make_mut(&mut meta).secondary_records.push(
        crate::metastream::RecordIndexEntry {
            entity_id: 41,
            bulk_offset: 61,
        },
    );
    let result = crate::test_support::with_decode_context(|ctx| {
        MeshOperationRecords::index(View::over_retained(&stream), meta).group_bodies(ctx, 8)
    })
    .unwrap();
    assert_eq!(
        result,
        Err(MeshOperationFraming("source member body reference"))
    );
}

#[test]
fn scope_validation_requires_the_operation_type_and_version() {
    let (stream, mut meta) = source_stream(1, &[(41, 9)]);
    let row = &mut std::sync::Arc::make_mut(&mut meta).types[0];
    row.type_guid = "4AB7A6BD-C40C-4C44-B7CA-0D415B6E4412"
        .to_owned()
        .try_into()
        .unwrap();
    row.base_type_guid = crate::records::entity_header::BaseTypeGuid::Guid {
        value: "90B8D2C6-05BB-4D0F-89EF-83AAC46CA1FF"
            .to_owned()
            .try_into()
            .unwrap(),
        offset: 0,
    };
    let records = MeshOperationRecords::index(View::over_retained(&stream), meta.clone());
    assert_eq!(
        records
            .validate_scope(MeshOperationKind::ReverseNormals, 8)
            .map(View::window),
        Ok(&stream[..36])
    );
    assert!(records
        .validate_scope(MeshOperationKind::Section, 8)
        .is_err());
    std::sync::Arc::make_mut(&mut meta).types[0].version = 1;
    assert!(
        MeshOperationRecords::index(View::over_retained(&stream), meta)
            .validate_scope(MeshOperationKind::ReverseNormals, 8)
            .is_err()
    );
}

#[test]
fn smooth_controls_survive_a_truncated_geometry_tail() {
    let mut scope = record(7, 34);
    put(&mut scope, 21, &reference(11));
    let read = read_scope(MeshOperationKind::Smooth, &scope).unwrap();
    assert_eq!(
        read.fields,
        MeshOperationFields::Smooth {
            smoothness_owner: 11
        }
    );
    assert_eq!(
        read.tail_error,
        Some(MeshOperationFraming("input geometry reference"))
    );
}

#[test]
fn null_smoothness_owner_retains_geometry_and_source_references() {
    let mut scope = record(7, 60);
    tail(&mut scope, 22, 30, 20, 8);
    let read = read_scope(MeshOperationKind::Smooth, &scope).unwrap();
    assert_eq!(
        read.fields,
        MeshOperationFields::Smooth {
            smoothness_owner: 0
        }
    );
    assert_eq!(read.output_geometry, Some(30));
    assert_eq!(read.input_geometry, 20);
    assert_eq!(read.source_group, 8);
    assert_eq!(read.tail_error, None);
}

#[test]
fn oversized_mesh_group_count_retains_readable_members_without_admitting_complete_selection() {
    let (stream, meta) = source_stream(u32::MAX, &[(11, 21), (12, 22)]);
    let records = MeshOperationRecords::index(View::over_retained(&stream), meta);
    crate::test_support::with_decode_context(|ctx| {
        let captured = records.group_body_references(ctx, 8).unwrap();
        assert_eq!(captured.declared_count, Some(u32::MAX));
        assert_eq!(captured.member_references, [11, 12]);
        assert_eq!(captured.references, [21, 22]);
        assert_eq!(
            captured.framing,
            Some(MeshOperationFraming("source group member count"))
        );
        assert_eq!(
            records.group_bodies(ctx, 8).unwrap(),
            Err(MeshOperationFraming("source group member count"))
        );
    });
}
