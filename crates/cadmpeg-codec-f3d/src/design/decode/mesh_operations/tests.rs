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
        read_scope(MeshOperationKind::ReverseNormals, &scope[..40]),
        Err(MeshOperationFraming("input geometry reference"))
    );
    let mut null_input = scope;
    put(&mut null_input, 32, &[0; 11]);
    assert_eq!(
        read_scope(MeshOperationKind::ReverseNormals, &null_input),
        Err(MeshOperationFraming("input geometry reference"))
    );
}

fn source_stream(count: u32, members: &[(u32, u32)]) -> Vec<u8> {
    let mut stream = record(8, 25 + 11 * members.len());
    put(&mut stream, 21, &count.to_le_bytes());
    for (ordinal, (member, _)) in members.iter().enumerate() {
        put(&mut stream, 25 + 11 * ordinal, &reference(*member));
    }
    for (member, body) in members {
        let mut bytes = record(*member, 40);
        put(&mut bytes, 21, &reference(*body));
        stream.extend_from_slice(&bytes);
    }
    stream
}

#[test]
fn source_group_members_name_their_bodies_in_group_order() {
    let stream = source_stream(2, &[(41, 9), (40, 3)]);
    let bodies = crate::test_support::with_decode_context(|ctx| {
        MeshOperationRecords::index(&stream).group_bodies(ctx, 8)
    })
    .unwrap();
    assert_eq!(bodies, Ok(vec![9, 3]));
}

#[test]
fn source_group_member_count_is_bounded_before_allocation() {
    for (count, members) in [(u32::MAX, vec![(41, 9)]), (0, vec![])] {
        let stream = source_stream(count, &members);
        let bodies = crate::test_support::with_decode_context(|ctx| {
            MeshOperationRecords::index(&stream).group_bodies(ctx, 8)
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
    let stream = source_stream(1, &[(41, 9)]);
    let records = MeshOperationRecords::index(&stream[..36]);
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
