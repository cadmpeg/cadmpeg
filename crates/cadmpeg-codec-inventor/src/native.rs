// SPDX-License-Identifier: Apache-2.0
//! Typed Inventor-native structural records.

use serde::{Deserialize, Serialize};

/// Current Inventor native namespace version.
pub(crate) const INVENTOR_NATIVE_VERSION: u32 = 25;

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
#[serde(try_from = "ProteinRecordWire", into = "ProteinRecordWire")]
pub(crate) enum ProteinRecord {
    Absent {
        id: String,
    },
    Empty {
        id: String,
        directory_id: u32,
    },
    Package {
        id: String,
        directory_id: u32,
        declared_len: std::num::NonZeroU32,
        entry_count: u64,
    },
    Malformed {
        id: String,
        directory_id: u32,
        detail: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProteinRecordState {
    Absent,
    Empty,
    Package,
    Malformed,
}

#[derive(Serialize, Deserialize)]
struct ProteinRecordWire {
    id: String,
    state: ProteinRecordState,
    directory_id: Option<u32>,
    declared_len: Option<u32>,
    entry_count: u64,
    detail: Option<String>,
}

impl From<ProteinRecord> for ProteinRecordWire {
    fn from(value: ProteinRecord) -> Self {
        match value {
            ProteinRecord::Absent { id } => Self {
                id,
                state: ProteinRecordState::Absent,
                directory_id: None,
                declared_len: None,
                entry_count: 0,
                detail: None,
            },
            ProteinRecord::Empty { id, directory_id } => Self {
                id,
                state: ProteinRecordState::Empty,
                directory_id: Some(directory_id),
                declared_len: Some(0),
                entry_count: 0,
                detail: None,
            },
            ProteinRecord::Package {
                id,
                directory_id,
                declared_len,
                entry_count,
            } => Self {
                id,
                state: ProteinRecordState::Package,
                directory_id: Some(directory_id),
                declared_len: Some(declared_len.get()),
                entry_count,
                detail: None,
            },
            ProteinRecord::Malformed {
                id,
                directory_id,
                detail,
            } => Self {
                id,
                state: ProteinRecordState::Malformed,
                directory_id: Some(directory_id),
                declared_len: None,
                entry_count: 0,
                detail: Some(detail),
            },
        }
    }
}

impl TryFrom<ProteinRecordWire> for ProteinRecord {
    type Error = String;

    fn try_from(wire: ProteinRecordWire) -> Result<Self, Self::Error> {
        match wire.state {
            ProteinRecordState::Absent => Ok(Self::Absent { id: wire.id }),
            ProteinRecordState::Empty => Ok(Self::Empty {
                id: wire.id,
                directory_id: wire
                    .directory_id
                    .ok_or_else(|| "empty Protein requires directory_id".to_owned())?,
            }),
            ProteinRecordState::Package => Ok(Self::Package {
                id: wire.id,
                directory_id: wire
                    .directory_id
                    .ok_or_else(|| "Protein package requires directory_id".to_owned())?,
                declared_len: std::num::NonZeroU32::new(wire.declared_len.unwrap_or(0))
                    .ok_or_else(|| "Protein package declared_len must be nonzero".to_owned())?,
                entry_count: wire.entry_count,
            }),
            ProteinRecordState::Malformed => Ok(Self::Malformed {
                id: wire.id,
                directory_id: wire
                    .directory_id
                    .ok_or_else(|| "malformed Protein requires directory_id".to_owned())?,
                detail: wire
                    .detail
                    .ok_or_else(|| "malformed Protein requires detail".to_owned())?,
            }),
        }
    }
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
#[serde(try_from = "UfrxRecordWire", into = "UfrxRecordWire")]
pub(crate) enum UfrxRecord {
    Absent {
        id: String,
    },
    ParsedPrefix {
        id: String,
        directory_id: u32,
        schema: u16,
        section_versions: Vec<u16>,
        original_file_name: String,
        caption: String,
        representation: Option<UfrxRepresentationRecord>,
        model_state_count: u64,
        reference_count: u64,
        embedded_reference_count: u64,
        occurrence_count: u64,
        tail_len: u64,
        tail_sha256: String,
    },
    Unsupported {
        id: String,
        directory_id: u32,
        schema: u16,
        section_versions: Vec<u16>,
        tail_len: u64,
        tail_sha256: String,
        detail: String,
    },
    Malformed {
        id: String,
        directory_id: u32,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UfrxRepresentationRecord {
    pub(crate) prefix: u16,
    pub(crate) active_representation: Option<String>,
    pub(crate) active_representation_kind: Option<String>,
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
enum UfrxRecordState {
    Absent,
    ParsedPrefix,
    Unsupported,
    Malformed,
}

#[derive(Serialize, Deserialize)]
struct UfrxRecordWire {
    id: String,
    state: UfrxRecordState,
    directory_id: Option<u32>,
    schema: Option<u16>,
    section_versions: Vec<u16>,
    original_file_name: Option<String>,
    caption: Option<String>,
    representation: Option<UfrxRepresentationRecord>,
    model_state_count: u64,
    reference_count: u64,
    embedded_reference_count: u64,
    occurrence_count: u64,
    tail_len: u64,
    tail_sha256: Option<String>,
    detail: Option<String>,
}

impl From<UfrxRecord> for UfrxRecordWire {
    fn from(value: UfrxRecord) -> Self {
        match value {
            UfrxRecord::Absent { id } => Self {
                id,
                state: UfrxRecordState::Absent,
                directory_id: None,
                schema: None,
                section_versions: Vec::new(),
                original_file_name: None,
                caption: None,
                representation: None,
                model_state_count: 0,
                reference_count: 0,
                embedded_reference_count: 0,
                occurrence_count: 0,
                tail_len: 0,
                tail_sha256: None,
                detail: None,
            },
            UfrxRecord::ParsedPrefix {
                id,
                directory_id,
                schema,
                section_versions,
                original_file_name,
                caption,
                representation,
                model_state_count,
                reference_count,
                embedded_reference_count,
                occurrence_count,
                tail_len,
                tail_sha256,
            } => Self {
                id,
                state: UfrxRecordState::ParsedPrefix,
                directory_id: Some(directory_id),
                schema: Some(schema),
                section_versions,
                original_file_name: Some(original_file_name),
                caption: Some(caption),
                representation,
                model_state_count,
                reference_count,
                embedded_reference_count,
                occurrence_count,
                tail_len,
                tail_sha256: Some(tail_sha256),
                detail: None,
            },
            UfrxRecord::Unsupported {
                id,
                directory_id,
                schema,
                section_versions,
                tail_len,
                tail_sha256,
                detail,
            } => Self {
                id,
                state: UfrxRecordState::Unsupported,
                directory_id: Some(directory_id),
                schema: Some(schema),
                section_versions,
                original_file_name: None,
                caption: None,
                representation: None,
                model_state_count: 0,
                reference_count: 0,
                embedded_reference_count: 0,
                occurrence_count: 0,
                tail_len,
                tail_sha256: Some(tail_sha256),
                detail: Some(detail),
            },
            UfrxRecord::Malformed {
                id,
                directory_id,
                detail,
            } => Self {
                id,
                state: UfrxRecordState::Malformed,
                directory_id: Some(directory_id),
                schema: None,
                section_versions: Vec::new(),
                original_file_name: None,
                caption: None,
                representation: None,
                model_state_count: 0,
                reference_count: 0,
                embedded_reference_count: 0,
                occurrence_count: 0,
                tail_len: 0,
                tail_sha256: None,
                detail: Some(detail),
            },
        }
    }
}

impl TryFrom<UfrxRecordWire> for UfrxRecord {
    type Error = String;

    fn try_from(wire: UfrxRecordWire) -> Result<Self, Self::Error> {
        match wire.state {
            UfrxRecordState::Absent => Ok(Self::Absent { id: wire.id }),
            UfrxRecordState::ParsedPrefix => Ok(Self::ParsedPrefix {
                id: wire.id,
                directory_id: wire
                    .directory_id
                    .ok_or_else(|| "parsed UFRxDoc requires directory_id".to_owned())?,
                schema: wire
                    .schema
                    .ok_or_else(|| "parsed UFRxDoc requires schema".to_owned())?,
                section_versions: wire.section_versions,
                original_file_name: wire
                    .original_file_name
                    .ok_or_else(|| "parsed UFRxDoc requires original_file_name".to_owned())?,
                caption: wire
                    .caption
                    .ok_or_else(|| "parsed UFRxDoc requires caption".to_owned())?,
                representation: wire.representation,
                model_state_count: wire.model_state_count,
                reference_count: wire.reference_count,
                embedded_reference_count: wire.embedded_reference_count,
                occurrence_count: wire.occurrence_count,
                tail_len: wire.tail_len,
                tail_sha256: wire
                    .tail_sha256
                    .ok_or_else(|| "parsed UFRxDoc requires tail_sha256".to_owned())?,
            }),
            UfrxRecordState::Unsupported => Ok(Self::Unsupported {
                id: wire.id,
                directory_id: wire
                    .directory_id
                    .ok_or_else(|| "unsupported UFRxDoc requires directory_id".to_owned())?,
                schema: wire
                    .schema
                    .ok_or_else(|| "unsupported UFRxDoc requires schema".to_owned())?,
                section_versions: wire.section_versions,
                tail_len: wire.tail_len,
                tail_sha256: wire
                    .tail_sha256
                    .ok_or_else(|| "unsupported UFRxDoc requires tail_sha256".to_owned())?,
                detail: wire
                    .detail
                    .ok_or_else(|| "unsupported UFRxDoc requires detail".to_owned())?,
            }),
            UfrxRecordState::Malformed => Ok(Self::Malformed {
                id: wire.id,
                directory_id: wire
                    .directory_id
                    .ok_or_else(|| "malformed UFRxDoc requires directory_id".to_owned())?,
                detail: wire
                    .detail
                    .ok_or_else(|| "malformed UFRxDoc requires detail".to_owned())?,
            }),
        }
    }
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmGraphicsFaceRecord {
    pub(crate) id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) segment_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) flags: u32,
    pub(crate) styles_reference: u32,
    pub(crate) styles_reference_qualified: bool,
    pub(crate) surface_reference: u32,
    pub(crate) surface_reference_qualified: bool,
    pub(crate) parent_reference: u32,
    pub(crate) parent_reference_qualified: bool,
    pub(crate) state: u32,
    pub(crate) edge_references: Vec<u32>,
    pub(crate) edge_reference_qualifiers: Vec<bool>,
    pub(crate) edge_list_metadata: Option<[u32; 2]>,
    pub(crate) visibility_state: u8,
    pub(crate) bounds: [f64; 6],
    pub(crate) key: u32,
    pub(crate) values: [u32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmGraphicsStyleCollectionRecord {
    pub(crate) id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) segment_version_major: u8,
    pub(crate) style_references: Vec<u32>,
    pub(crate) style_reference_qualifiers: Vec<bool>,
    pub(crate) list_metadata: Option<[u32; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PmGraphicsPrimaryColorStyleRecord {
    pub(crate) id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) segment_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) controls: [u16; 7],
    pub(crate) color_header: [u8; 2],
    pub(crate) colors: [[f32; 4]; 4],
    pub(crate) color_tail: [u16; 2],
    pub(crate) state: u8,
    pub(crate) values: [u16; 2],
    pub(crate) terminal_state: u8,
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
#[serde(try_from = "SegmentBulkRecordWire", into = "SegmentBulkRecordWire")]
pub(crate) struct SegmentBulkRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) prefix: String,
    pub(crate) form: u16,
    pub(crate) compressed_len: u64,
    pub(crate) compressed_sha256: String,
    pub(crate) expanded_len: u64,
    pub(crate) expanded_sha256: String,
    pub(crate) records: SegmentBulkFrame,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SegmentBulkFrame {
    Framed {
        record_count: u64,
        stream_trailer_len: u64,
        stream_trailer_sha256: String,
    },
    Unavailable {
        detail: String,
    },
    NotExpanded,
}

#[derive(Serialize, Deserialize)]
struct SegmentBulkRecordWire {
    id: String,
    token: String,
    prefix: String,
    form: u16,
    compressed_len: u64,
    compressed_sha256: String,
    expanded_len: Option<u64>,
    expanded_sha256: Option<String>,
    record_state: String,
    record_count: u64,
    stream_trailer_len: Option<u64>,
    stream_trailer_sha256: Option<String>,
    record_detail: Option<String>,
}

impl From<SegmentBulkRecord> for SegmentBulkRecordWire {
    fn from(value: SegmentBulkRecord) -> Self {
        let (record_state, record_count, stream_trailer_len, stream_trailer_sha256, record_detail) =
            match value.records {
                SegmentBulkFrame::Framed {
                    record_count,
                    stream_trailer_len,
                    stream_trailer_sha256,
                } => (
                    "framed".into(),
                    record_count,
                    Some(stream_trailer_len),
                    Some(stream_trailer_sha256),
                    None,
                ),
                SegmentBulkFrame::Unavailable { detail } => {
                    ("unavailable".into(), 0, None, None, Some(detail))
                }
                SegmentBulkFrame::NotExpanded => ("not_expanded".into(), 0, None, None, None),
            };
        Self {
            id: value.id,
            token: value.token,
            prefix: value.prefix,
            form: value.form,
            compressed_len: value.compressed_len,
            compressed_sha256: value.compressed_sha256,
            expanded_len: Some(value.expanded_len),
            expanded_sha256: Some(value.expanded_sha256),
            record_state,
            record_count,
            stream_trailer_len,
            stream_trailer_sha256,
            record_detail,
        }
    }
}

impl TryFrom<SegmentBulkRecordWire> for SegmentBulkRecord {
    type Error = String;

    fn try_from(wire: SegmentBulkRecordWire) -> Result<Self, Self::Error> {
        let expanded_len = wire.expanded_len.unwrap_or(0);
        let expanded_sha256 = wire.expanded_sha256.unwrap_or_default();
        let records = match wire.record_state.as_str() {
            "framed" => SegmentBulkFrame::Framed {
                record_count: wire.record_count,
                stream_trailer_len: wire
                    .stream_trailer_len
                    .ok_or_else(|| "framed bulk requires stream_trailer_len".to_owned())?,
                stream_trailer_sha256: wire
                    .stream_trailer_sha256
                    .ok_or_else(|| "framed bulk requires stream_trailer_sha256".to_owned())?,
            },
            "unavailable" => SegmentBulkFrame::Unavailable {
                detail: wire
                    .record_detail
                    .ok_or_else(|| "unavailable bulk requires record_detail".to_owned())?,
            },
            "not_expanded" => SegmentBulkFrame::NotExpanded,
            other => return Err(format!("unknown bulk record_state {other}")),
        };
        Ok(Self {
            id: wire.id,
            token: wire.token,
            prefix: wire.prefix,
            form: wire.form,
            compressed_len: wire.compressed_len,
            compressed_sha256: wire.compressed_sha256,
            expanded_len,
            expanded_sha256,
            records,
        })
    }
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
#[serde(try_from = "ActiveCarrierRecordWire", into = "ActiveCarrierRecordWire")]
pub(crate) enum ActiveCarrierRecord {
    NotApplicable {
        id: String,
    },
    NotExpanded {
        id: String,
    },
    Unavailable {
        id: String,
        detail: String,
    },
    Selected {
        id: String,
        segment_token: String,
        record_ordinal: u32,
        segment_version_major: u8,
        family: crate::kernel::KernelFamily,
        header_state: u32,
        header_kind: u16,
        header_value: u32,
        schema: u32,
        carrier_len: std::num::NonZeroU64,
        carrier_offset: u64,
        carrier_sha256: String,
        selected_key: u32,
        enabled: bool,
        delta_state: i32,
        history_reference: u32,
    },
}

#[derive(Serialize, Deserialize)]
struct ActiveCarrierRecordWire {
    id: String,
    state: ActiveCarrierRecordState,
    segment_token: Option<String>,
    record_ordinal: Option<u32>,
    segment_version_major: Option<u8>,
    family: Option<String>,
    header_state: Option<u32>,
    header_kind: Option<u16>,
    header_value: Option<u32>,
    schema: Option<u32>,
    carrier_len: Option<u64>,
    carrier_offset: Option<u64>,
    carrier_sha256: Option<String>,
    selected_key: Option<u32>,
    enabled: Option<bool>,
    delta_state: Option<i32>,
    history_reference: Option<u32>,
    detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ActiveCarrierRecordState {
    NotApplicable,
    NotExpanded,
    Selected,
    Unavailable,
}

impl From<ActiveCarrierRecord> for ActiveCarrierRecordWire {
    fn from(value: ActiveCarrierRecord) -> Self {
        match value {
            ActiveCarrierRecord::NotApplicable { id } => Self {
                id,
                state: ActiveCarrierRecordState::NotApplicable,
                segment_token: None,
                record_ordinal: None,
                segment_version_major: None,
                family: None,
                header_state: None,
                header_kind: None,
                header_value: None,
                schema: None,
                carrier_len: None,
                carrier_offset: None,
                carrier_sha256: None,
                selected_key: None,
                enabled: None,
                delta_state: None,
                history_reference: None,
                detail: None,
            },
            ActiveCarrierRecord::NotExpanded { id } => Self {
                id,
                state: ActiveCarrierRecordState::NotExpanded,
                segment_token: None,
                record_ordinal: None,
                segment_version_major: None,
                family: None,
                header_state: None,
                header_kind: None,
                header_value: None,
                schema: None,
                carrier_len: None,
                carrier_offset: None,
                carrier_sha256: None,
                selected_key: None,
                enabled: None,
                delta_state: None,
                history_reference: None,
                detail: None,
            },
            ActiveCarrierRecord::Unavailable { id, detail } => Self {
                id,
                state: ActiveCarrierRecordState::Unavailable,
                segment_token: None,
                record_ordinal: None,
                segment_version_major: None,
                family: None,
                header_state: None,
                header_kind: None,
                header_value: None,
                schema: None,
                carrier_len: None,
                carrier_offset: None,
                carrier_sha256: None,
                selected_key: None,
                enabled: None,
                delta_state: None,
                history_reference: None,
                detail: Some(detail),
            },
            ActiveCarrierRecord::Selected {
                id,
                segment_token,
                record_ordinal,
                segment_version_major,
                family,
                header_state,
                header_kind,
                header_value,
                schema,
                carrier_len,
                carrier_offset,
                carrier_sha256,
                selected_key,
                enabled,
                delta_state,
                history_reference,
            } => Self {
                id,
                state: ActiveCarrierRecordState::Selected,
                segment_token: Some(segment_token),
                record_ordinal: Some(record_ordinal),
                segment_version_major: Some(segment_version_major),
                family: Some(family.label().into()),
                header_state: Some(header_state),
                header_kind: Some(header_kind),
                header_value: Some(header_value),
                schema: Some(schema),
                carrier_len: Some(carrier_len.get()),
                carrier_offset: Some(carrier_offset),
                carrier_sha256: Some(carrier_sha256),
                selected_key: Some(selected_key),
                enabled: Some(enabled),
                delta_state: Some(delta_state),
                history_reference: Some(history_reference),
                detail: None,
            },
        }
    }
}

impl TryFrom<ActiveCarrierRecordWire> for ActiveCarrierRecord {
    type Error = String;

    fn try_from(wire: ActiveCarrierRecordWire) -> Result<Self, Self::Error> {
        match wire.state {
            ActiveCarrierRecordState::NotApplicable => Ok(Self::NotApplicable { id: wire.id }),
            ActiveCarrierRecordState::NotExpanded => Ok(Self::NotExpanded { id: wire.id }),
            ActiveCarrierRecordState::Unavailable => {
                let detail = wire
                    .detail
                    .ok_or_else(|| "unavailable active carrier requires detail".to_owned())?;
                Ok(Self::Unavailable {
                    id: wire.id,
                    detail,
                })
            }
            ActiveCarrierRecordState::Selected => {
                let family = match wire.family.as_deref() {
                    Some("asm") => crate::kernel::KernelFamily::Asm,
                    Some("acis") => crate::kernel::KernelFamily::Acis,
                    other => {
                        return Err(format!(
                            "selected active carrier family must be asm or acis, got {other:?}"
                        ))
                    }
                };
                let carrier_len = std::num::NonZeroU64::new(wire.carrier_len.unwrap_or(0))
                    .ok_or_else(|| "selected active carrier_len must be nonzero".to_owned())?;
                Ok(Self::Selected {
                    id: wire.id,
                    segment_token: wire.segment_token.ok_or_else(|| {
                        "selected active carrier requires segment_token".to_owned()
                    })?,
                    record_ordinal: wire.record_ordinal.ok_or_else(|| {
                        "selected active carrier requires record_ordinal".to_owned()
                    })?,
                    segment_version_major: wire.segment_version_major.ok_or_else(|| {
                        "selected active carrier requires segment_version_major".to_owned()
                    })?,
                    family,
                    header_state: wire.header_state.ok_or_else(|| {
                        "selected active carrier requires header_state".to_owned()
                    })?,
                    header_kind: wire
                        .header_kind
                        .ok_or_else(|| "selected active carrier requires header_kind".to_owned())?,
                    header_value: wire.header_value.ok_or_else(|| {
                        "selected active carrier requires header_value".to_owned()
                    })?,
                    schema: wire
                        .schema
                        .ok_or_else(|| "selected active carrier requires schema".to_owned())?,
                    carrier_len,
                    carrier_offset: wire.carrier_offset.ok_or_else(|| {
                        "selected active carrier requires carrier_offset".to_owned()
                    })?,
                    carrier_sha256: wire.carrier_sha256.ok_or_else(|| {
                        "selected active carrier requires carrier_sha256".to_owned()
                    })?,
                    selected_key: wire.selected_key.ok_or_else(|| {
                        "selected active carrier requires selected_key".to_owned()
                    })?,
                    enabled: wire
                        .enabled
                        .ok_or_else(|| "selected active carrier requires enabled".to_owned())?,
                    delta_state: wire
                        .delta_state
                        .ok_or_else(|| "selected active carrier requires delta_state".to_owned())?,
                    history_reference: wire.history_reference.ok_or_else(|| {
                        "selected active carrier requires history_reference".to_owned()
                    })?,
                })
            }
        }
    }
}

impl ActiveCarrierRecord {
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::NotApplicable { id }
            | Self::NotExpanded { id }
            | Self::Unavailable { id, .. }
            | Self::Selected { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SegmentBulkIssueRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) detail: String,
}
