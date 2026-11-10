use super::*;
use crate::design::decode::mesh::{
    mesh_body_frame_losses, mesh_body_identity_losses, mesh_face_group_losses,
    mesh_history_window_losses, mesh_operation_record_losses, mesh_split_stream_losses,
    mesh_wrapper_tail_losses,
};

fn notes(notes: impl Iterator<Item = cadmpeg_ir::report::loss::LossNote>) -> Vec<(String, String)> {
    notes
        .map(|note| (note.code.local_code().to_owned(), note.message))
        .collect()
}

fn note(code: &str, message: &str) -> Vec<(String, String)> {
    vec![(code.to_owned(), message.to_owned())]
}

fn replace_frame(
    graph: &mut SyntheticMeshGraph,
    type_guid: &str,
    entity_id: u64,
    replace: impl FnOnce(&[u8]) -> Vec<u8>,
) {
    let frame = typed_primary_frames(&graph.bytes, &graph.meta, type_guid, "test")
        .unwrap()
        .into_iter()
        .find(|frame| frame.entity_id == entity_id)
        .unwrap();
    let (start, end) = (frame.start, frame.end);
    let bytes = replace(&graph.bytes[start..end]);
    let delta = i64::try_from(bytes.len()).unwrap() - i64::try_from(end - start).unwrap();
    graph.bytes.splice(start..end, bytes);
    for record in &mut graph.meta.records {
        if record.bulk_offset > u64_from_index(start) {
            record.bulk_offset = record.bulk_offset.checked_add_signed(delta).unwrap();
        }
    }
}

fn compact_mesh_body(old: &[u8]) -> Vec<u8> {
    let mut body = old[..mesh_body::FIRST_PRESENCE].to_vec();
    body[20] = 1;
    body.splice(21..21, [0; 4]);
    body.extend_from_slice(&[1, 1]);
    body.extend_from_slice(&1.0_f32.to_le_bytes());
    body.extend_from_slice(&[1, 1]);
    body.extend_from_slice(&[0; 9]);
    lp_utf16(&mut body, "11111111-2222-4333-8444-555555555555");
    lp_utf16(&mut body, "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C");
    body.extend_from_slice(&[0; 4]);
    lp_utf16(&mut body, "PrismMaterial-018");
    body.extend_from_slice(
        &old[mesh_body::FEATURE_SCOPE_REFERENCE
            ..mesh_body::CONTAINER_GUID_REFERENCE + SAME_SEGMENT_REFERENCE_BYTES],
    );
    body.push(0);
    body.extend_from_slice(&old[mesh_body::SCENE_NODE_REFERENCE..mesh_body::LEN]);
    lp_utf16(&mut body, "Cut");
    body.extend_from_slice(&old[old.len() - SAME_SEGMENT_REFERENCE_BYTES..]);
    body
}

#[test]
fn operation_bodies_report_their_compact_matrix_frame_and_wrapper_tail() {
    let mut graph = super::collection::operation_body_graph();
    replace_frame(&mut graph, MESH_BODY_TYPE_GUID, 117, compact_mesh_body);
    replace_frame(&mut graph, MESH_WRAPPER_TYPE_GUID, 121, |old| {
        [old, &[9, 8, 7]].concat()
    });
    let features = parse_mesh_design_records(
        &graph.bytes,
        &graph.meta,
        "Synthetic/BulkStream.dat",
        &mut no_texture_asset,
    )
    .expect("operation graph");
    let [operation] = features[0].operation_bodies.as_slice() else {
        panic!("one operation body");
    };
    assert_eq!(operation.body.placement.record().record_index(), 117);
    assert_eq!(
        notes(mesh_body_identity_losses(&features)),
        note(
            "mesh.body-identity-assumed",
            "mesh-body record 117 reads a compact matrix as identity"
        )
    );
    assert_eq!(
        notes(mesh_body_frame_losses(&features)),
        note(
            "mesh.body-bytes-uninterpreted",
            "mesh-body record 117 retains its frame with uninterpreted bytes"
        )
    );
    assert_eq!(
        notes(mesh_wrapper_tail_losses(&features)),
        note(
            "mesh.wrapper-bytes-uninterpreted",
            "mesh-wrapper record 121 retains its uninterpreted tail"
        )
    );
    assert_eq!(
        notes(mesh_operation_record_losses(&features)),
        note(
            "mesh.operation-records-native",
            "mesh operation scope 122 retains its source records and input GUID links in the native record"
        )
    );
}

#[test]
fn stream_and_face_group_emitters_name_each_retained_record() {
    let graph = synthetic_mesh_graph(false);
    let mut features = parse_mesh_design_records(
        &graph.bytes,
        &graph.meta,
        "Synthetic/BulkStream.dat",
        &mut no_texture_asset,
    )
    .expect("mesh graph");
    assert!(notes(mesh_split_stream_losses(&features)).is_empty());
    assert!(notes(mesh_history_window_losses(&features)).is_empty());
    assert!(notes(mesh_face_group_losses(&features)).is_empty());
    let body = features[0].body_at_mut(0).unwrap();
    body.split_streams
        .push(crate::records::mesh::DesignMeshSplitStream {
            name: "v".into(),
            descriptor: cadmpeg_ir::native::bytes::NativeBytes::new(vec![0x84]),
        });
    body.history_streams
        .push(crate::records::mesh::DesignMeshHistoryStream {
            name: "t".into(),
            encoding: cadmpeg_ir::native::bytes::NativeBytes::new([0x5d, 0x14]),
            decoded_length: 1_052_800,
        });
    body.face_group_metadata = Some(crate::records::mesh::DesignMeshFaceGroups {
        declared_count: 3,
        entries: vec![crate::records::mesh::DesignMeshFaceGroup {
            key: 0,
            guid: "d46db2ae-2a8b-4624-b83c-bcb6cbce247f".into(),
        }],
    });
    assert_eq!(
        notes(mesh_split_stream_losses(&features)),
        note(
            "mesh.split-stream-assumed",
            "mesh body 104 stream v reads its component segments in stored order; the native record retains the descriptor"
        )
    );
    assert_eq!(
        notes(mesh_history_window_losses(&features)),
        note(
            "mesh.history-window-assumed",
            "mesh body 104 stream t uses its declared output length as the LZMA history bound; the native record retains its encoding"
        )
    );
    assert_eq!(
        notes(mesh_face_group_losses(&features)),
        note(
            "mesh.face-groups-assumed",
            "mesh body 104 uses its face-group entries and triangle selectors; the native record retains declared count 3 and all 1 entries"
        )
    );
}
