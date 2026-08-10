// SPDX-License-Identifier: Apache-2.0
//! Typed Inventor-native structural records.

use serde::{Deserialize, Serialize};

/// Current Inventor native namespace version.
pub(crate) const INVENTOR_NATIVE_VERSION: u32 = 5;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UfrxRecord {
    pub(crate) id: String,
    pub(crate) state: UfrxRecordState,
    pub(crate) directory_id: Option<u32>,
    pub(crate) schema: Option<u16>,
    pub(crate) section_versions: Vec<u16>,
    pub(crate) original_file_name: Option<String>,
    pub(crate) caption: Option<String>,
    pub(crate) reference_count: u64,
    pub(crate) tail_len: u64,
    pub(crate) tail_sha256: Option<String>,
    pub(crate) detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UfrxRecordState {
    Absent,
    ParsedPrefix,
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
    pub(crate) header_words: [u32; 4],
    pub(crate) state_words: [u32; 3],
    pub(crate) created: String,
    pub(crate) modified: String,
    pub(crate) body_form: u8,
    pub(crate) expanded_body_len: u64,
    pub(crate) expanded_body_sha256: String,
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
    pub(crate) expanded_len: u64,
    pub(crate) expanded_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SegmentBulkIssueRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) detail: String,
}
