// SPDX-License-Identifier: Apache-2.0
//! Synthetic Design mesh graph records.

use cadmpeg_core::CodecError;
use crate::design::decode::mesh::{COMMON_DATA_MODULE, DATA_MODEL_MODULE, FUSION_MODULE, MATRIX_BYTES, MESH_BODY_BASE_TYPE_GUID, MESH_BODY_OWNER_BASE_TYPE_GUID, MESH_BODY_OWNER_TYPE_GUID, MESH_BODY_OWNER_TYPE_VERSION, MESH_BODY_TYPE_GUID, MESH_BODY_TYPE_VERSION, MESH_COLLECTION_BASE_BASE_TYPE_GUID, MESH_COLLECTION_BASE_TYPE_GUID, MESH_COLLECTION_BASE_TYPE_VERSION, MESH_COLLECTION_OWNER_BASE_TYPE_GUID, MESH_COLLECTION_OWNER_TYPE_GUID, MESH_COLLECTION_OWNER_TYPE_VERSIONS, MESH_COLLECTION_TYPE_GUID, MESH_COLLECTION_TYPE_VERSION, MESH_ENTRY_NAME_BASE_TYPE_GUID, MESH_ENTRY_NAME_TYPE_GUID, MESH_ENTRY_NAME_TYPE_VERSION, MESH_FEATURE_SCOPE_BASE_TYPE_GUID, MESH_FEATURE_SCOPE_TYPE_GUID, MESH_FEATURE_SCOPE_TYPE_VERSION, MESH_GUID_BASE_TYPE_GUID, MESH_GUID_TYPE_GUID, MESH_GUID_TYPE_VERSION, MESH_SCENE_STATE_BASE_TYPE_GUID, MESH_SCENE_STATE_TYPE_GUID, MESH_SCENE_STATE_TYPE_VERSION, MESH_SCOPE_BASE_RECORD_BASE_TYPE_GUID, MESH_SCOPE_BASE_RECORD_TYPE_GUID, MESH_SCOPE_BASE_RECORD_TYPE_VERSION, MESH_TEXTURE_FILENAME_BASE_TYPE_GUID, MESH_TEXTURE_FILENAME_TYPE_GUID, MESH_TEXTURE_FILENAME_TYPE_VERSION, MESH_TEXTURE_TABLE_BASE_TYPE_GUID, MESH_TEXTURE_TABLE_TYPE_GUID, MESH_TEXTURE_TABLE_TYPE_VERSION, MESH_WRAPPER_BASE_TYPE_GUID, MESH_WRAPPER_TYPE_GUID, MESH_WRAPPER_TYPE_VERSION, PARAMESH_MODULE, SAME_SEGMENT_REFERENCE_BYTES, SCENE_AUXILIARY_BASE_TYPE_GUID, SCENE_AUXILIARY_TYPE_GUID, SCENE_AUXILIARY_TYPE_VERSION, SCENE_MODULE, SCENE_NODE_BASE_TYPE_GUID, SCENE_NODE_TYPE_GUID, SCENE_NODE_TYPE_VERSION};
use crate::design::test_support::{design_type, primary_record};
use crate::layout::{paramesh_collection_owner_backlink_prefix as collection_owner, paramesh_mesh_body_join_prefix as mesh_body};
use crate::test_support::{lp_ascii, lp_utf16};

pub(crate) fn matrix(cells: [f64; 16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(MATRIX_BYTES);
    for value in cells {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

pub(crate) fn push_indexed_header(bytes: &mut Vec<u8>, class_tag: u32, record_index: u32) {
    let class_tag = class_tag.to_string();
    assert_eq!(class_tag.len(), 3);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(class_tag.as_bytes());
    bytes.extend_from_slice(&record_index.to_le_bytes());
}

pub(crate) fn put_reference(bytes: &mut [u8], at: usize, target: u32) {
    bytes[at] = 1;
    bytes[at + 1..at + 9].copy_from_slice(&u64::from(target).to_le_bytes());
    bytes[at + 9..at + 11].copy_from_slice(&[0, 0]);
}

pub(crate) fn push_reference(bytes: &mut Vec<u8>, target: u32) {
    let at = bytes.len();
    bytes.resize(at + SAME_SEGMENT_REFERENCE_BYTES, 0);
    put_reference(bytes, at, target);
}

fn mesh_entry_record(
    class_tag: u32,
    record_index: u32,
    guid_record_index: u32,
    entry_name: &str,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, class_tag, record_index);
    bytes.extend_from_slice(&[0; 10]);
    push_reference(&mut bytes, guid_record_index);
    lp_utf16(&mut bytes, entry_name);
    bytes
}

fn mesh_guid_record(
    class_tag: u32,
    record_index: u32,
    entry_name_record_index: u32,
    fusion_uuid: &str,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, class_tag, record_index);
    bytes.extend_from_slice(&[0; 21]);
    lp_ascii(&mut bytes, fusion_uuid);
    push_reference(&mut bytes, entry_name_record_index);
    bytes.extend_from_slice(&[0; 4]);
    bytes
}

#[allow(clippy::too_many_arguments)] // One argument per serialized body-graph reference.
fn mesh_body_record(
    class_tag: u32,
    record_index: u32,
    guid_record_index: u32,
    scope_record_index: u32,
    wrapper_record_index: u32,
    owner_record_index: u32,
    scene_node_record_index: u32,
    collection_record_index: u32,
) -> Vec<u8> {
    let identity = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, class_tag, record_index);
    bytes.resize(600, 0);
    bytes[mesh_body::FIRST_TRANSFORM..mesh_body::FIRST_TRANSFORM + MATRIX_BYTES]
        .copy_from_slice(&matrix(identity));
    bytes[mesh_body::SECOND_TRANSFORM..mesh_body::SECOND_TRANSFORM + MATRIX_BYTES]
        .copy_from_slice(&matrix(identity));
    put_reference(
        &mut bytes,
        mesh_body::FEATURE_SCOPE_REFERENCE,
        scope_record_index,
    );
    put_reference(
        &mut bytes,
        mesh_body::WRAPPER_REFERENCE,
        wrapper_record_index,
    );
    put_reference(
        &mut bytes,
        mesh_body::BODY_OWNER_REFERENCE,
        owner_record_index,
    );
    put_reference(
        &mut bytes,
        mesh_body::CONTAINER_GUID_REFERENCE,
        guid_record_index,
    );
    put_reference(
        &mut bytes,
        mesh_body::SCENE_NODE_REFERENCE,
        scene_node_record_index,
    );
    let tail_at = bytes.len() - SAME_SEGMENT_REFERENCE_BYTES;
    put_reference(&mut bytes, tail_at, collection_record_index);
    bytes
}

fn push_scene_footer(bytes: &mut Vec<u8>) {
    bytes.push(1);
    for index in 0..49 {
        bytes.push(match index {
            6 | 14 | 22 | 30 | 38 | 46 => 0xef,
            31 | 39 | 47 => 0x7f,
            48 => 0x01,
            _ => 0xff,
        });
    }
}

/// Prefix and owner forms of a mesh collection.
#[derive(Clone, Copy)]
pub(crate) enum CollectionForm {
    Plain,
    /// A zero u32 before the owner reference.
    OwnerGap,
    /// Flag byte `01` at offset 20 and this u32 at offset 25.
    Flagged(u32),
}

fn mesh_collection_record(
    class_tag: u32,
    base_class_tag: u32,
    record_index: u32,
    texture_table_record_index: u32,
    body_record_indices: &[u32],
    owner_record_index: u32,
    form: CollectionForm,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, class_tag, record_index);
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(u8::from(matches!(form, CollectionForm::Flagged(_))));
    bytes.extend_from_slice(
        &(u32::try_from(body_record_indices.len()).expect("fixture value fits u32"))
            .to_le_bytes(),
    );
    if let CollectionForm::Flagged(word) = form {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(&[1, 1]);
    push_reference(&mut bytes, texture_table_record_index);
    push_indexed_header(&mut bytes, base_class_tag, record_index);
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(
        &(u32::try_from(body_record_indices.len()).expect("fixture value fits u32"))
            .to_le_bytes(),
    );
    for body in body_record_indices {
        push_reference(&mut bytes, *body);
    }
    if matches!(form, CollectionForm::OwnerGap) {
        bytes.extend_from_slice(&[0; 4]);
    }
    push_reference(&mut bytes, owner_record_index);
    bytes
}

pub(crate) fn mesh_texture_table_record(
    class_tag: u32,
    record_index: u32,
    flags: &[(&str, u32)],
    filenames: &[(&str, u32)],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, class_tag, record_index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(
        &(u32::try_from(flags.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for (guid, value) in flags {
        lp_ascii(&mut bytes, guid);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(
        &(u32::try_from(filenames.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    for (guid, target) in filenames {
        lp_ascii(&mut bytes, guid);
        push_reference(&mut bytes, *target);
    }
    bytes
}

pub(crate) fn mesh_wrapper_record(class_tag: u32, record_index: u32, body_record_index: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, class_tag, record_index);
    bytes.extend_from_slice(&[0; 10]);
    push_reference(&mut bytes, body_record_index);
    bytes.extend_from_slice(&[0; 8]);
    bytes
}

fn mesh_scene_state_record(class_tag: u32, record_index: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, class_tag, record_index);
    bytes.extend_from_slice(&[0; 34]);
    push_scene_footer(&mut bytes);
    bytes
}

fn mesh_scene_node_record(
    class_tag: u32,
    record_index: u32,
    state_record_index: u32,
    auxiliary_record_index: u32,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, class_tag, record_index);
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    push_reference(&mut bytes, state_record_index);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    push_reference(&mut bytes, auxiliary_record_index);
    bytes.extend_from_slice(&[0; 24]);
    push_scene_footer(&mut bytes);
    bytes
}

pub(crate) fn placed_mesh_scene_node_record(
    class_tag: u32,
    record_index: u32,
    state_record_index: u32,
    auxiliary_record_index: u32,
    transform: [f64; 16],
    bounds: [f64; 6],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, class_tag, record_index);
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    push_reference(&mut bytes, state_record_index);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    push_reference(&mut bytes, auxiliary_record_index);
    bytes.extend_from_slice(&[0; 25]);
    for value in transform {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in bounds {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(1);
    bytes
}

fn mesh_scope_record(
    class_tag: u32,
    base_class_tag: u32,
    record_index: u32,
    body_record_indices: &[u32],
    member_record_index: u32,
    owner_record_index: u32,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, class_tag, record_index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(
        &(u32::try_from(body_record_indices.len()).expect("fixture value fits u32"))
            .to_le_bytes(),
    );
    for body in body_record_indices {
        push_reference(&mut bytes, *body);
    }
    bytes.extend_from_slice(&[0; 24]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    push_reference(&mut bytes, member_record_index);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Base Mesh Feature");
    let mut tail = [0; 78];
    tail[..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    push_indexed_header(&mut bytes, base_class_tag, record_index);
    bytes.extend_from_slice(&[0; 8]);
    push_reference(&mut bytes, owner_record_index);
    bytes
}

fn collection_owner_record(
    class_tag: u32,
    record_index: u32,
    collection_record_index: u32,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, class_tag, record_index);
    bytes.resize(collection_owner::COLLECTION_BACKLINK, 0);
    push_reference(&mut bytes, collection_record_index);
    bytes
}

fn identity_record(class_tag: u32, record_index: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_indexed_header(&mut bytes, class_tag, record_index);
    bytes
}

fn texture_filename_record(class_tag: u32, record_index: u32, filename: &str) -> Vec<u8> {
    let mut bytes = identity_record(class_tag, record_index);
    bytes.extend_from_slice(&[0; 10]);
    lp_utf16(&mut bytes, filename);
    bytes
}

fn push_primary_frame(
    bytes: &mut Vec<u8>,
    records: &mut Vec<crate::metastream::RecordIndexEntry>,
    entity_id: u32,
    frame: Vec<u8>,
) {
    records.push(primary_record(u64::from(entity_id), bytes.len()));
    bytes.extend(frame);
}

pub(crate) struct SyntheticMeshGraph {
    pub(crate) bytes: Vec<u8>,
    pub(crate) meta: crate::metastream::MetaStream,
}

pub(crate) fn no_texture_asset(
    _filename: &str,
) -> Result<(String, cadmpeg_ir::assets::AssetId), CodecError> {
    panic!("empty texture table must not resolve an asset")
}

pub(crate) fn synthetic_mesh_graph_with_body_count(
    with_textures: bool,
    body_count: usize,
) -> SyntheticMeshGraph {
    synthetic_mesh_graph_with_collection(with_textures, body_count, CollectionForm::Plain)
}

pub(crate) fn synthetic_mesh_graph_with_collection(
    with_textures: bool,
    body_count: usize,
    form: CollectionForm,
) -> SyntheticMeshGraph {
    const ENTRY: u32 = 102;
    const GUID: u32 = 103;
    const BODY: u32 = 104;
    const STATE: u32 = 105;
    const AUXILIARY: u32 = 106;
    const NODE: u32 = 107;
    const WRAPPER: u32 = 108;
    const SCOPE: u32 = 109;
    const COLLECTION_OWNER: u32 = 110;
    const BODY_OWNER: u32 = 111;
    const SCOPE_OWNER: u32 = 112;
    const FILENAME_A: u32 = 113;
    const FILENAME_B: u32 = 114;
    const SECOND_ENTRY: u32 = 115;
    const SECOND_GUID: u32 = 116;
    const SECOND_BODY: u32 = 117;
    const SECOND_STATE: u32 = 118;
    const SECOND_AUXILIARY: u32 = 119;
    const SECOND_NODE: u32 = 120;
    const SECOND_WRAPPER: u32 = 121;
    const COLLECTION: u32 = 100;
    const TEXTURES: u32 = 101;
    const ENTRIES: [u32; 2] = [ENTRY, SECOND_ENTRY];
    const GUIDS: [u32; 2] = [GUID, SECOND_GUID];
    const BODIES: [u32; 2] = [BODY, SECOND_BODY];
    const STATES: [u32; 2] = [STATE, SECOND_STATE];
    const AUXILIARIES: [u32; 2] = [AUXILIARY, SECOND_AUXILIARY];
    const NODES: [u32; 2] = [NODE, SECOND_NODE];
    const WRAPPERS: [u32; 2] = [WRAPPER, SECOND_WRAPPER];
    const ENTRY_NAMES: [&str; 2] = [
        "ParaMeshGeometry.11111111-2222-4333-8444-555555555555.paramesh",
        "ParaMeshGeometry.66666666-7777-4888-8999-AAAAAAAAAAAA.paramesh",
    ];
    const FUSION_UUIDS: [&str; 2] = [
        "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE",
        "BBBBBBBB-CCCC-4DDD-8EEE-FFFFFFFFFFFF",
    ];
    const RESOURCE_A: &str = "10000000-0000-4000-8000-000000000001";
    const RESOURCE_B: &str = "20000000-0000-4000-8000-000000000002";

    const TAG_ENTRY: u32 = 256;
    const TAG_GUID: u32 = 257;
    const TAG_BODY: u32 = 258;
    const TAG_COLLECTION: u32 = 259;
    const TAG_COLLECTION_BASE: u32 = 260;
    const TAG_TEXTURES: u32 = 261;
    const TAG_WRAPPER: u32 = 262;
    const TAG_SCOPE: u32 = 263;
    const TAG_SCOPE_BASE: u32 = 264;
    const TAG_STATE: u32 = 265;
    const TAG_NODE: u32 = 266;
    const TAG_AUXILIARY: u32 = 267;
    const TAG_COLLECTION_OWNER: u32 = 268;
    const TAG_BODY_OWNER: u32 = 269;
    const TAG_FILENAME: u32 = 270;

    assert!(body_count <= 2);
    let entity_ids = |indices: &[u32]| {
        indices[..body_count]
            .iter()
            .copied()
            .map(u64::from)
            .collect()
    };

    let filename_ids = if with_textures {
        vec![u64::from(FILENAME_A), u64::from(FILENAME_B)]
    } else {
        Vec::new()
    };
    let types = vec![
        design_type(
            256,
            MESH_ENTRY_NAME_TYPE_GUID,
            Some(MESH_ENTRY_NAME_BASE_TYPE_GUID),
            MESH_ENTRY_NAME_TYPE_VERSION,
            PARAMESH_MODULE,
            entity_ids(&ENTRIES),
        ),
        design_type(
            257,
            MESH_GUID_TYPE_GUID,
            Some(MESH_GUID_BASE_TYPE_GUID),
            MESH_GUID_TYPE_VERSION,
            PARAMESH_MODULE,
            entity_ids(&GUIDS),
        ),
        design_type(
            258,
            MESH_BODY_TYPE_GUID,
            Some(MESH_BODY_BASE_TYPE_GUID),
            MESH_BODY_TYPE_VERSION,
            PARAMESH_MODULE,
            entity_ids(&BODIES),
        ),
        design_type(
            259,
            MESH_COLLECTION_TYPE_GUID,
            Some(MESH_COLLECTION_BASE_TYPE_GUID),
            MESH_COLLECTION_TYPE_VERSION,
            PARAMESH_MODULE,
            vec![u64::from(COLLECTION)],
        ),
        design_type(
            260,
            MESH_COLLECTION_BASE_TYPE_GUID,
            Some(MESH_COLLECTION_BASE_BASE_TYPE_GUID),
            MESH_COLLECTION_BASE_TYPE_VERSION,
            COMMON_DATA_MODULE,
            Vec::new(),
        ),
        design_type(
            261,
            MESH_TEXTURE_TABLE_TYPE_GUID,
            Some(MESH_TEXTURE_TABLE_BASE_TYPE_GUID),
            MESH_TEXTURE_TABLE_TYPE_VERSION,
            PARAMESH_MODULE,
            vec![u64::from(TEXTURES)],
        ),
        design_type(
            262,
            MESH_WRAPPER_TYPE_GUID,
            Some(MESH_WRAPPER_BASE_TYPE_GUID),
            MESH_WRAPPER_TYPE_VERSION,
            PARAMESH_MODULE,
            entity_ids(&WRAPPERS),
        ),
        design_type(
            263,
            MESH_FEATURE_SCOPE_TYPE_GUID,
            Some(MESH_FEATURE_SCOPE_BASE_TYPE_GUID),
            MESH_FEATURE_SCOPE_TYPE_VERSION,
            FUSION_MODULE,
            vec![u64::from(SCOPE)],
        ),
        design_type(
            264,
            MESH_SCOPE_BASE_RECORD_TYPE_GUID,
            Some(MESH_SCOPE_BASE_RECORD_BASE_TYPE_GUID),
            MESH_SCOPE_BASE_RECORD_TYPE_VERSION,
            DATA_MODEL_MODULE,
            Vec::new(),
        ),
        design_type(
            265,
            MESH_SCENE_STATE_TYPE_GUID,
            Some(MESH_SCENE_STATE_BASE_TYPE_GUID),
            MESH_SCENE_STATE_TYPE_VERSION,
            SCENE_MODULE,
            entity_ids(&STATES),
        ),
        design_type(
            266,
            SCENE_NODE_TYPE_GUID,
            Some(SCENE_NODE_BASE_TYPE_GUID),
            SCENE_NODE_TYPE_VERSION,
            SCENE_MODULE,
            entity_ids(&NODES),
        ),
        design_type(
            267,
            SCENE_AUXILIARY_TYPE_GUID,
            Some(SCENE_AUXILIARY_BASE_TYPE_GUID),
            SCENE_AUXILIARY_TYPE_VERSION,
            SCENE_MODULE,
            entity_ids(&AUXILIARIES),
        ),
        design_type(
            268,
            MESH_COLLECTION_OWNER_TYPE_GUID,
            Some(MESH_COLLECTION_OWNER_BASE_TYPE_GUID),
            MESH_COLLECTION_OWNER_TYPE_VERSIONS[2],
            FUSION_MODULE,
            vec![u64::from(COLLECTION_OWNER)],
        ),
        design_type(
            269,
            MESH_BODY_OWNER_TYPE_GUID,
            Some(MESH_BODY_OWNER_BASE_TYPE_GUID),
            MESH_BODY_OWNER_TYPE_VERSION,
            "Body",
            vec![u64::from(BODY_OWNER)],
        ),
        design_type(
            270,
            MESH_TEXTURE_FILENAME_TYPE_GUID,
            Some(MESH_TEXTURE_FILENAME_BASE_TYPE_GUID),
            MESH_TEXTURE_FILENAME_TYPE_VERSION,
            "",
            filename_ids,
        ),
    ];
    assert_eq!(types.len(), 15);

    let mut bytes = Vec::new();
    let mut records = Vec::new();
    push_primary_frame(
        &mut bytes,
        &mut records,
        COLLECTION,
        mesh_collection_record(
            TAG_COLLECTION,
            TAG_COLLECTION_BASE,
            COLLECTION,
            TEXTURES,
            &BODIES[..body_count],
            COLLECTION_OWNER,
            form,
        ),
    );
    let flags = if with_textures {
        vec![(RESOURCE_A, 2), (RESOURCE_B, 258)]
    } else {
        Vec::new()
    };
    let filenames = if with_textures {
        vec![(RESOURCE_B, FILENAME_B), (RESOURCE_A, FILENAME_A)]
    } else {
        Vec::new()
    };
    push_primary_frame(
        &mut bytes,
        &mut records,
        TEXTURES,
        mesh_texture_table_record(TAG_TEXTURES, TEXTURES, &flags, &filenames),
    );
    if with_textures {
        push_primary_frame(
            &mut bytes,
            &mut records,
            FILENAME_A,
            texture_filename_record(TAG_FILENAME, FILENAME_A, "mesh-a.png"),
        );
        push_primary_frame(
            &mut bytes,
            &mut records,
            FILENAME_B,
            texture_filename_record(TAG_FILENAME, FILENAME_B, "mesh-b.jpg"),
        );
    }
    for ordinal in 0..body_count {
        push_primary_frame(
            &mut bytes,
            &mut records,
            ENTRIES[ordinal],
            mesh_entry_record(
                TAG_ENTRY,
                ENTRIES[ordinal],
                GUIDS[ordinal],
                ENTRY_NAMES[ordinal],
            ),
        );
        push_primary_frame(
            &mut bytes,
            &mut records,
            GUIDS[ordinal],
            mesh_guid_record(
                TAG_GUID,
                GUIDS[ordinal],
                ENTRIES[ordinal],
                FUSION_UUIDS[ordinal],
            ),
        );
        push_primary_frame(
            &mut bytes,
            &mut records,
            STATES[ordinal],
            mesh_scene_state_record(TAG_STATE, STATES[ordinal]),
        );
        push_primary_frame(
            &mut bytes,
            &mut records,
            AUXILIARIES[ordinal],
            identity_record(TAG_AUXILIARY, AUXILIARIES[ordinal]),
        );
        push_primary_frame(
            &mut bytes,
            &mut records,
            NODES[ordinal],
            mesh_scene_node_record(
                TAG_NODE,
                NODES[ordinal],
                STATES[ordinal],
                AUXILIARIES[ordinal],
            ),
        );
        push_primary_frame(
            &mut bytes,
            &mut records,
            WRAPPERS[ordinal],
            mesh_wrapper_record(TAG_WRAPPER, WRAPPERS[ordinal], BODIES[ordinal]),
        );
        push_primary_frame(
            &mut bytes,
            &mut records,
            BODIES[ordinal],
            mesh_body_record(
                TAG_BODY,
                BODIES[ordinal],
                GUIDS[ordinal],
                SCOPE,
                WRAPPERS[ordinal],
                BODY_OWNER,
                NODES[ordinal],
                COLLECTION,
            ),
        );
    }
    let scope_frame = mesh_scope_record(
        TAG_SCOPE,
        TAG_SCOPE_BASE,
        SCOPE,
        &BODIES[..body_count],
        140,
        SCOPE_OWNER,
    );
    let paired_scope = bytes.len() + scope_frame.len() - crate::layout::paramesh_feature_scope_base::LEN;
    push_primary_frame(&mut bytes, &mut records, SCOPE, scope_frame);
    push_primary_frame(
        &mut bytes,
        &mut records,
        COLLECTION_OWNER,
        collection_owner_record(TAG_COLLECTION_OWNER, COLLECTION_OWNER, COLLECTION),
    );
    push_primary_frame(
        &mut bytes,
        &mut records,
        BODY_OWNER,
        identity_record(TAG_BODY_OWNER, BODY_OWNER),
    );
    SyntheticMeshGraph {
        bytes,
        meta: crate::metastream::MetaStream {
            types,
            records,
            secondary_records: vec![primary_record(u64::from(SCOPE), paired_scope)],
        },
    }
}

pub(crate) fn synthetic_mesh_graph(with_textures: bool) -> SyntheticMeshGraph {
    synthetic_mesh_graph_with_body_count(with_textures, 1)
}

