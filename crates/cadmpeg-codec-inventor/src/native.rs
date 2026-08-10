// SPDX-License-Identifier: Apache-2.0
//! Typed Inventor-native structural records.

use serde::{Deserialize, Serialize};

/// Current Inventor native namespace version.
pub(crate) const INVENTOR_NATIVE_VERSION: u32 = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct VersionTupleRecord {
    pub(crate) revision: u8,
    pub(crate) minor: u8,
    pub(crate) major: u8,
    pub(crate) state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DatabaseRecord {
    pub(crate) id: String,
    pub(crate) band: u32,
    pub(crate) database_id: String,
    pub(crate) schema: u32,
    pub(crate) created_by: VersionTupleRecord,
    pub(crate) created_filetime: u64,
    pub(crate) saved_by: VersionTupleRecord,
    pub(crate) saved_filetime: u64,
    pub(crate) note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DatabaseIssueRecord {
    pub(crate) id: String,
    pub(crate) band: u32,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SegmentRegistryRecord {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) display_name: String,
    pub(crate) segment_id: String,
    pub(crate) revision_id: String,
    pub(crate) type_name: String,
    pub(crate) object_count: u64,
    pub(crate) node_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RevisionRecord {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) revision_id: String,
    pub(crate) flags: u32,
    pub(crate) kind: u16,
    pub(crate) payload_form: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StructuralIssueRecord {
    pub(crate) id: String,
    pub(crate) scope: String,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PropertySetRecord {
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) directory_id: u32,
    pub(crate) version: u16,
    pub(crate) system_identifier: u32,
    pub(crate) clsid: String,
    pub(crate) section_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PropertyRecord {
    pub(crate) id: String,
    pub(crate) set_path: String,
    pub(crate) section_ordinal: u32,
    pub(crate) fmtid: String,
    pub(crate) property_id: u32,
    pub(crate) name: Option<String>,
    pub(crate) type_code: Option<u16>,
    pub(crate) value_kind: String,
    pub(crate) scalar_value: Option<String>,
    pub(crate) raw_len: u64,
    pub(crate) raw_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PropertySectionRecord {
    pub(crate) id: String,
    pub(crate) set_path: String,
    pub(crate) ordinal: u32,
    pub(crate) fmtid: String,
    pub(crate) code_page: Option<u16>,
    pub(crate) offsets_ordered: bool,
    pub(crate) dictionary_entries: u64,
    pub(crate) property_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PropertySetIssueRecord {
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) directory_id: u32,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProteinRecord {
    pub(crate) id: String,
    pub(crate) state: ProteinRecordState,
    pub(crate) directory_id: Option<u32>,
    pub(crate) declared_len: Option<u32>,
    pub(crate) entry_count: u64,
    pub(crate) detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProteinRecordState {
    Absent,
    Empty,
    Package,
    Malformed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProteinEntryRecord {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) name: String,
    pub(crate) compression: String,
    pub(crate) crc32: u32,
    pub(crate) compressed_size: u64,
    pub(crate) uncompressed_size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ProteinAssetRecord {
    pub(crate) id: String,
    pub(crate) entry_name: String,
    pub(crate) ordinal: u64,
    pub(crate) asset: cadmpeg_protein::DecodedRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProteinRejectionRecord {
    pub(crate) id: String,
    pub(crate) entry_name: String,
    pub(crate) ordinal: u64,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UfrxRecord {
    pub(crate) id: String,
    pub(crate) state: UfrxRecordState,
    pub(crate) directory_id: Option<u32>,
    pub(crate) schema: Option<u16>,
    pub(crate) section_versions: Vec<u16>,
    pub(crate) original_file_name: Option<String>,
    pub(crate) caption: Option<String>,
    pub(crate) representation: Option<UfrxRepresentationRecord>,
    pub(crate) model_state_count: u64,
    pub(crate) reference_count: u64,
    pub(crate) embedded_reference_count: u64,
    pub(crate) occurrence_count: u64,
    pub(crate) tail_len: u64,
    pub(crate) tail_sha256: Option<String>,
    pub(crate) detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UfrxRepresentationRecord {
    pub(crate) prefix: u16,
    pub(crate) active_representation: String,
    pub(crate) active_representation_kind: String,
    pub(crate) secondary_active_lod_state: [u16; 2],
    pub(crate) active_model_state: String,
    pub(crate) active_model_state_state: [u16; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UfrxModelStateRecord {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) prefix: u8,
    pub(crate) name: String,
    pub(crate) state: [u16; 2],
    pub(crate) prefix_count: u32,
    pub(crate) parameters: Vec<UfrxModelStateParameterRecord>,
    pub(crate) suffix_len: u64,
    pub(crate) suffix_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UfrxModelStateParameterRecord {
    pub(crate) name: String,
    pub(crate) tag: u8,
    pub(crate) kind: u16,
    pub(crate) state: u16,
    pub(crate) value: String,
    pub(crate) trailer: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UfrxRecordState {
    Absent,
    ParsedPrefix,
    Unsupported,
    Malformed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ExternalReferenceRecord {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) path: String,
    pub(crate) library_id: i32,
    pub(crate) library_name: String,
    pub(crate) display_name: String,
    pub(crate) state_groups: Vec<[u16; 3]>,
    pub(crate) state: [u16; 2],
    pub(crate) document_id: String,
    pub(crate) database_id: String,
    pub(crate) reference_id: u32,
    pub(crate) occurrence_count: u32,
    pub(crate) version: u32,
    pub(crate) flags: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EmbeddedReferenceRecord {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) value_0: u32,
    pub(crate) filetime: u64,
    pub(crate) value_1: u32,
    pub(crate) extended_value: Option<u32>,
    pub(crate) value_2: u32,
    pub(crate) path: String,
    pub(crate) library_id: i32,
    pub(crate) library_name: String,
    pub(crate) state: u16,
    pub(crate) display_name: String,
    pub(crate) state_values: [u8; 8],
    pub(crate) record_len: u64,
    pub(crate) record_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UfrxOccurrenceRecord {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) end_string_flag: u32,
    pub(crate) file_reference_id: u32,
    pub(crate) occurrence_id: u32,
    pub(crate) header_value: u32,
    pub(crate) title: Option<String>,
    pub(crate) header_padding_words: u8,
    pub(crate) record_len: u64,
    pub(crate) record_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AssemblyOccurrenceRecord {
    pub(crate) id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) next_reference: u32,
    pub(crate) flags: u32,
    pub(crate) owner_reference: u32,
    pub(crate) node_index: u32,
    pub(crate) state: [i32; 2],
    pub(crate) ordinal_key: u32,
    pub(crate) related_references: Vec<u32>,
    pub(crate) child_reference: u32,
    pub(crate) occurrence_id: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AssemblyPlacementRecord {
    pub(crate) id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) header_id: u16,
    pub(crate) owner_reference: u32,
    pub(crate) attribute_reference: u32,
    pub(crate) state: u8,
    pub(crate) transform_prefix: bool,
    pub(crate) transform_encoding: [u16; 2],
    pub(crate) transform: [[f64; 4]; 4],
    pub(crate) branch: u8,
    pub(crate) graphics_state: u8,
    pub(crate) occurrence_id: u32,
    pub(crate) graphics_index: u32,
    pub(crate) object_reference: u32,
    pub(crate) suffix_len: u64,
    pub(crate) suffix_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AssemblyRecordIssueRecord {
    pub(crate) id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmAppDefaultStyleRecord {
    pub(crate) id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) segment_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) material_reference: u32,
    pub(crate) rendering_style_reference: u32,
    pub(crate) related_references: [u32; 7],
    pub(crate) state: u8,
    pub(crate) terminal_reference: u32,
    pub(crate) suffix_len: u64,
    pub(crate) suffix_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmAppRenderingStyleRecord {
    pub(crate) id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) segment_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) state: u8,
    pub(crate) flags: u16,
    pub(crate) values: [u16; 2],
    pub(crate) default_state: u32,
    pub(crate) value: u32,
    pub(crate) name_reference: u32,
    pub(crate) name: String,
    pub(crate) comment: String,
    pub(crate) long_name: String,
    pub(crate) style_state: Option<u16>,
    pub(crate) style_label: Option<String>,
    pub(crate) asset_guid: Option<String>,
    pub(crate) material_id: Option<String>,
    pub(crate) asset_library_id: Option<String>,
    pub(crate) style_values: Option<[u16; 2]>,
    pub(crate) guid: Option<String>,
    pub(crate) suffix_len: u64,
    pub(crate) suffix_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PresentationRecordIssueRecord {
    pub(crate) id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StorageBandRecord {
    pub(crate) id: String,
    pub(crate) band: u32,
    pub(crate) database_directory_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SegmentPairRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) metadata_directory_id: u32,
    pub(crate) bulk_directory_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UnpairedSegmentRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) missing_member: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SegmentMetaRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) version: u16,
    pub(crate) kind: String,
    pub(crate) display_name: String,
    pub(crate) segment_id: String,
    pub(crate) header_values: [u16; 8],
    pub(crate) state_words: [u32; 3],
    pub(crate) created: String,
    pub(crate) modified: String,
    pub(crate) body_form: u8,
    pub(crate) expanded_body_len: u64,
    pub(crate) expanded_body_sha256: String,
    pub(crate) table_prefix: [u16; 7],
    pub(crate) block_count: u64,
    pub(crate) type_count: u64,
    pub(crate) terminal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MetaSectionRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) number: u8,
    pub(crate) discriminator: u32,
    pub(crate) payload_len: u64,
    pub(crate) payload_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MetaTypeRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) index: u8,
    pub(crate) type_id: String,
    pub(crate) fields: [(u16, u32); 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SegmentMetaIssueRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) status: String,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SegmentBulkRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) prefix: String,
    pub(crate) form: u16,
    pub(crate) compressed_len: u64,
    pub(crate) compressed_sha256: String,
    pub(crate) expanded_len: Option<u64>,
    pub(crate) expanded_sha256: Option<String>,
    pub(crate) record_state: String,
    pub(crate) record_count: u64,
    pub(crate) stream_trailer_len: Option<u64>,
    pub(crate) stream_trailer_sha256: Option<String>,
    pub(crate) record_detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RseRecordRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) ordinal: u32,
    pub(crate) selector: u32,
    pub(crate) type_index: u8,
    pub(crate) type_id: String,
    pub(crate) payload_offset: u64,
    pub(crate) payload_len: u64,
    pub(crate) payload_sha256: String,
    pub(crate) trailing_payload_len: u32,
    pub(crate) trailer_len: u64,
    pub(crate) trailer_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ActiveCarrierRecord {
    pub(crate) id: String,
    pub(crate) state: ActiveCarrierRecordState,
    pub(crate) segment_token: Option<String>,
    pub(crate) record_ordinal: Option<u32>,
    pub(crate) segment_version_major: Option<u8>,
    pub(crate) family: Option<String>,
    pub(crate) header_state: Option<u32>,
    pub(crate) header_kind: Option<u16>,
    pub(crate) header_value: Option<u32>,
    pub(crate) schema: Option<u32>,
    pub(crate) carrier_len: Option<u64>,
    pub(crate) carrier_offset: Option<u64>,
    pub(crate) carrier_sha256: Option<String>,
    pub(crate) selected_key: Option<u32>,
    pub(crate) enabled: Option<bool>,
    pub(crate) delta_state: Option<i32>,
    pub(crate) history_reference: Option<u32>,
    pub(crate) detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActiveCarrierRecordState {
    NotApplicable,
    NotExpanded,
    Selected,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SegmentBulkIssueRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) detail: String,
}
