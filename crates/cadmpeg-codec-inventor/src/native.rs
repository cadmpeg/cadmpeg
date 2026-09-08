// SPDX-License-Identifier: Apache-2.0
//! Typed Inventor-native structural records.

pub(crate) mod digest;
pub(crate) mod protein;
pub(crate) mod ufrx;

use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

use crate::pmdc::{PmDcPairedReferenceList, PmDcReference};
use crate::presentation::RenderingStyleExtension;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RevisionPayloadForm {
    None,
    Short,
    Long,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RevisionRecord {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) revision_id: String,
    pub(crate) flags: u32,
    pub(crate) kind: u16,
    pub(crate) payload_form: RevisionPayloadForm,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PropertyValueKind {
    Empty {
        type_code: u16,
    },
    Signed {
        type_code: u16,
    },
    Unsigned {
        type_code: u16,
    },
    Float {
        type_code: u16,
    },
    Bool {
        type_code: u16,
    },
    Filetime {
        type_code: u16,
    },
    String {
        type_code: u16,
    },
    Guid {
        type_code: u16,
    },
    Binary {
        type_code: u16,
        len: usize,
    },
    Clipboard {
        type_code: u16,
        format: u32,
        len: usize,
    },
    Vector {
        type_code: u16,
        len: usize,
    },
    Dictionary,
    Unknown {
        type_code: u16,
    },
}

impl PropertyValueKind {
    fn type_code(&self) -> Option<u16> {
        match self {
            Self::Dictionary => None,
            Self::Empty { type_code }
            | Self::Signed { type_code }
            | Self::Unsigned { type_code }
            | Self::Float { type_code }
            | Self::Bool { type_code }
            | Self::Filetime { type_code }
            | Self::String { type_code }
            | Self::Guid { type_code }
            | Self::Binary { type_code, .. }
            | Self::Clipboard { type_code, .. }
            | Self::Vector { type_code, .. }
            | Self::Unknown { type_code } => Some(*type_code),
        }
    }

    fn from_wire(text: &str, type_code: Option<u16>) -> Result<Self, String> {
        if text == "dictionary" {
            return match type_code {
                None => Ok(Self::Dictionary),
                Some(type_code) => Err(format!(
                    "dictionary property value_kind cannot carry type_code {type_code}"
                )),
            };
        }
        let Some(type_code) = type_code else {
            return Err(format!("property value_kind {text} requires type_code"));
        };
        let expected = crate::property_set::property_kind_name(type_code);
        if text.split(':').next() != Some(expected) {
            return Err(format!("property value_kind {text} disagrees with type_code {type_code}: expected {expected}"));
        }
        match text {
            "empty" => Ok(Self::Empty { type_code }),
            "signed" => Ok(Self::Signed { type_code }),
            "unsigned" => Ok(Self::Unsigned { type_code }),
            "float" => Ok(Self::Float { type_code }),
            "bool" => Ok(Self::Bool { type_code }),
            "filetime" => Ok(Self::Filetime { type_code }),
            "string" => Ok(Self::String { type_code }),
            "guid" => Ok(Self::Guid { type_code }),
            "unknown" => Ok(Self::Unknown { type_code }),
            text => {
                if let Some(len) = text.strip_prefix("binary:") {
                    let len = len
                        .parse()
                        .map_err(|_| format!("property value_kind {text}"))?;
                    return Ok(Self::Binary { type_code, len });
                }
                if let Some(rest) = text.strip_prefix("clipboard:") {
                    let (format, len) = rest
                        .split_once(':')
                        .ok_or_else(|| format!("property value_kind {text}"))?;
                    let format = format
                        .parse()
                        .map_err(|_| format!("property value_kind {text}"))?;
                    let len = len
                        .parse()
                        .map_err(|_| format!("property value_kind {text}"))?;
                    return Ok(Self::Clipboard {
                        type_code,
                        format,
                        len,
                    });
                }
                if let Some(len) = text.strip_prefix("vector:") {
                    let len = len
                        .parse()
                        .map_err(|_| format!("property value_kind {text}"))?;
                    return Ok(Self::Vector { type_code, len });
                }
                Err(format!("property value_kind {text}"))
            }
        }
    }
}

impl Display for PropertyValueKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty { .. } => formatter.write_str("empty"),
            Self::Signed { .. } => formatter.write_str("signed"),
            Self::Unsigned { .. } => formatter.write_str("unsigned"),
            Self::Float { .. } => formatter.write_str("float"),
            Self::Bool { .. } => formatter.write_str("bool"),
            Self::Filetime { .. } => formatter.write_str("filetime"),
            Self::String { .. } => formatter.write_str("string"),
            Self::Guid { .. } => formatter.write_str("guid"),
            Self::Binary { len, .. } => write!(formatter, "binary:{len}"),
            Self::Clipboard { format, len, .. } => {
                write!(formatter, "clipboard:{format}:{len}")
            }
            Self::Vector { len, .. } => write!(formatter, "vector:{len}"),
            Self::Dictionary => formatter.write_str("dictionary"),
            Self::Unknown { .. } => formatter.write_str("unknown"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PropertyRecordWire", into = "PropertyRecordWire")]
pub(crate) struct PropertyRecord {
    pub(crate) id: String,
    pub(crate) set_path: String,
    pub(crate) section_ordinal: u32,
    pub(crate) fmtid: String,
    pub(crate) property_id: u32,
    pub(crate) name: Option<String>,
    pub(crate) value_kind: PropertyValueKind,
    pub(crate) scalar_value: Option<String>,
    pub(crate) raw_len: u64,
    pub(crate) raw_sha256: String,
}

#[derive(Serialize, Deserialize)]
struct PropertyRecordWire {
    id: String,
    set_path: String,
    section_ordinal: u32,
    fmtid: String,
    property_id: u32,
    name: Option<String>,
    type_code: Option<u16>,
    value_kind: String,
    scalar_value: Option<String>,
    raw_len: u64,
    raw_sha256: String,
}

impl From<PropertyRecord> for PropertyRecordWire {
    fn from(value: PropertyRecord) -> Self {
        Self {
            type_code: value.value_kind.type_code(),
            value_kind: value.value_kind.to_string(),
            id: value.id,
            set_path: value.set_path,
            section_ordinal: value.section_ordinal,
            fmtid: value.fmtid,
            property_id: value.property_id,
            name: value.name,
            scalar_value: value.scalar_value,
            raw_len: value.raw_len,
            raw_sha256: value.raw_sha256,
        }
    }
}

impl TryFrom<PropertyRecordWire> for PropertyRecord {
    type Error = String;

    fn try_from(wire: PropertyRecordWire) -> Result<Self, Self::Error> {
        Ok(Self {
            value_kind: PropertyValueKind::from_wire(&wire.value_kind, wire.type_code)?,
            id: wire.id,
            set_path: wire.set_path,
            section_ordinal: wire.section_ordinal,
            fmtid: wire.fmtid,
            property_id: wire.property_id,
            name: wire.name,
            scalar_value: wire.scalar_value,
            raw_len: wire.raw_len,
            raw_sha256: wire.raw_sha256,
        })
    }
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
    #[serde(flatten, with = "crate::compact_matrix::assembly_wire")]
    pub(crate) transform: crate::compact_matrix::CompactMatrix,
    pub(crate) branch: u8,
    pub(crate) graphics_state: u8,
    pub(crate) occurrence_id: u32,
    pub(crate) graphics_index: u32,
    pub(crate) object_reference: u32,
    pub(crate) suffix_len: u64,
    pub(crate) suffix_sha256: String,
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
#[serde(
    try_from = "PmAppRenderingStyleRecordWire",
    into = "PmAppRenderingStyleRecordWire"
)]
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
    pub(crate) extension: Option<RenderingStyleExtension>,
    pub(crate) suffix_len: u64,
    pub(crate) suffix_sha256: String,
}

#[derive(Serialize, Deserialize)]
struct PmAppRenderingStyleRecordWire {
    id: String,
    segment_token: String,
    record_ordinal: u32,
    segment_version_major: u8,
    header_value: u32,
    header_id: u16,
    state: u8,
    flags: u16,
    values: [u16; 2],
    default_state: u32,
    value: u32,
    name_reference: u32,
    name: String,
    comment: String,
    long_name: String,
    style_state: Option<u16>,
    style_label: Option<String>,
    asset_guid: Option<String>,
    material_id: Option<String>,
    asset_library_id: Option<String>,
    style_values: Option<[u16; 2]>,
    guid: Option<String>,
    suffix_len: u64,
    suffix_sha256: String,
}

impl From<PmAppRenderingStyleRecord> for PmAppRenderingStyleRecordWire {
    fn from(value: PmAppRenderingStyleRecord) -> Self {
        let (
            style_state,
            style_label,
            asset_guid,
            material_id,
            asset_library_id,
            style_values,
            guid,
        ) = match value.extension {
            Some(extension) => (
                Some(extension.style_state),
                Some(extension.style_label),
                Some(extension.asset_guid),
                Some(extension.material_id),
                Some(extension.asset_library_id),
                Some(extension.style_values),
                Some(extension.guid),
            ),
            None => (None, None, None, None, None, None, None),
        };
        Self {
            id: value.id,
            segment_token: value.segment_token,
            record_ordinal: value.record_ordinal,
            segment_version_major: value.segment_version_major,
            header_value: value.header_value,
            header_id: value.header_id,
            state: value.state,
            flags: value.flags,
            values: value.values,
            default_state: value.default_state,
            value: value.value,
            name_reference: value.name_reference,
            name: value.name,
            comment: value.comment,
            long_name: value.long_name,
            style_state,
            style_label,
            asset_guid,
            material_id,
            asset_library_id,
            style_values,
            guid,
            suffix_len: value.suffix_len,
            suffix_sha256: value.suffix_sha256,
        }
    }
}

impl TryFrom<PmAppRenderingStyleRecordWire> for PmAppRenderingStyleRecord {
    type Error = String;

    fn try_from(wire: PmAppRenderingStyleRecordWire) -> Result<Self, Self::Error> {
        let extension = match (
            wire.style_state,
            wire.style_label,
            wire.asset_guid,
            wire.material_id,
            wire.asset_library_id,
            wire.style_values,
            wire.guid,
        ) {
            (None, None, None, None, None, None, None) => None,
            (
                Some(style_state),
                Some(style_label),
                Some(asset_guid),
                Some(material_id),
                Some(asset_library_id),
                Some(style_values),
                Some(guid),
            ) => Some(RenderingStyleExtension {
                style_state,
                style_label,
                asset_guid,
                material_id,
                asset_library_id,
                style_values,
                guid,
            }),
            _ => {
                return Err("rendering style extension fields must be present together".into());
            }
        };
        if extension.is_some() != (wire.segment_version_major >= 17) {
            return Err("rendering style extension disagrees with segment_version_major".into());
        }
        if wire.segment_version_major >= 17 && !wire.comment.is_empty() {
            return Err(
                "rendering style comment must be empty for segment_version_major >= 17".into(),
            );
        }
        Ok(Self {
            id: wire.id,
            segment_token: wire.segment_token,
            record_ordinal: wire.record_ordinal,
            segment_version_major: wire.segment_version_major,
            header_value: wire.header_value,
            header_id: wire.header_id,
            state: wire.state,
            flags: wire.flags,
            values: wire.values,
            default_state: wire.default_state,
            value: wire.value,
            name_reference: wire.name_reference,
            name: wire.name,
            comment: wire.comment,
            long_name: wire.long_name,
            extension,
            suffix_len: wire.suffix_len,
            suffix_sha256: wire.suffix_sha256,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    try_from = "PmGraphicsFaceRecordWire",
    into = "PmGraphicsFaceRecordWire"
)]
pub(crate) struct PmGraphicsFaceRecord {
    pub(crate) id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) segment_version_major: u8,
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) flags: u32,
    pub(crate) styles: PmDcReference,
    pub(crate) surface: PmDcReference,
    pub(crate) parent: PmDcReference,
    pub(crate) state: u32,
    pub(crate) edge_references: PmDcPairedReferenceList<[u32; 2]>,
    pub(crate) visibility_state: u8,
    pub(crate) bounds: [f64; 6],
    pub(crate) key: u32,
    pub(crate) values: [u32; 2],
}

#[derive(Serialize, Deserialize)]
struct PmGraphicsFaceRecordWire {
    id: String,
    segment_token: String,
    record_ordinal: u32,
    segment_version_major: u8,
    header_value: u32,
    header_id: u16,
    flags: u32,
    styles_reference: u32,
    styles_reference_qualified: bool,
    surface_reference: u32,
    surface_reference_qualified: bool,
    parent_reference: u32,
    parent_reference_qualified: bool,
    state: u32,
    edge_references: Vec<u32>,
    edge_reference_qualifiers: Vec<bool>,
    edge_list_metadata: Option<[u32; 2]>,
    visibility_state: u8,
    bounds: [f64; 6],
    key: u32,
    values: [u32; 2],
}

impl From<PmGraphicsFaceRecord> for PmGraphicsFaceRecordWire {
    fn from(value: PmGraphicsFaceRecord) -> Self {
        let (edge_references, edge_reference_qualifiers) =
            PmDcReference::unzip(value.edge_references.references());
        Self {
            id: value.id,
            segment_token: value.segment_token,
            record_ordinal: value.record_ordinal,
            segment_version_major: value.segment_version_major,
            header_value: value.header_value,
            header_id: value.header_id,
            flags: value.flags,
            styles_reference: value.styles.index,
            styles_reference_qualified: value.styles.qualified,
            surface_reference: value.surface.index,
            surface_reference_qualified: value.surface.qualified,
            parent_reference: value.parent.index,
            parent_reference_qualified: value.parent.qualified,
            state: value.state,
            edge_references,
            edge_reference_qualifiers,
            edge_list_metadata: value.edge_references.metadata().copied(),
            visibility_state: value.visibility_state,
            bounds: value.bounds,
            key: value.key,
            values: value.values,
        }
    }
}

impl TryFrom<PmGraphicsFaceRecordWire> for PmGraphicsFaceRecord {
    type Error = String;

    fn try_from(wire: PmGraphicsFaceRecordWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            segment_token: wire.segment_token,
            record_ordinal: wire.record_ordinal,
            segment_version_major: wire.segment_version_major,
            header_value: wire.header_value,
            header_id: wire.header_id,
            flags: wire.flags,
            styles: PmDcReference {
                index: wire.styles_reference,
                qualified: wire.styles_reference_qualified,
            },
            surface: PmDcReference {
                index: wire.surface_reference,
                qualified: wire.surface_reference_qualified,
            },
            parent: PmDcReference {
                index: wire.parent_reference,
                qualified: wire.parent_reference_qualified,
            },
            state: wire.state,
            edge_references: PmDcPairedReferenceList::new(
                wire.edge_list_metadata,
                PmDcReference::zip(wire.edge_references, wire.edge_reference_qualifiers)?,
            )
            .ok_or("edge_list_metadata disagrees with edge_references")?,
            visibility_state: wire.visibility_state,
            bounds: wire.bounds,
            key: wire.key,
            values: wire.values,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "PmGraphicsStyleCollectionRecordWire",
    into = "PmGraphicsStyleCollectionRecordWire"
)]
pub(crate) struct PmGraphicsStyleCollectionRecord {
    pub(crate) id: String,
    pub(crate) segment_token: String,
    pub(crate) record_ordinal: u32,
    pub(crate) segment_version_major: u8,
    pub(crate) style_references: PmDcPairedReferenceList<[u32; 2]>,
}

#[derive(Serialize, Deserialize)]
struct PmGraphicsStyleCollectionRecordWire {
    id: String,
    segment_token: String,
    record_ordinal: u32,
    segment_version_major: u8,
    style_references: Vec<u32>,
    style_reference_qualifiers: Vec<bool>,
    list_metadata: Option<[u32; 2]>,
}

impl From<PmGraphicsStyleCollectionRecord> for PmGraphicsStyleCollectionRecordWire {
    fn from(value: PmGraphicsStyleCollectionRecord) -> Self {
        let (style_references, style_reference_qualifiers) =
            PmDcReference::unzip(value.style_references.references());
        Self {
            id: value.id,
            segment_token: value.segment_token,
            record_ordinal: value.record_ordinal,
            segment_version_major: value.segment_version_major,
            style_references,
            style_reference_qualifiers,
            list_metadata: value.style_references.metadata().copied(),
        }
    }
}

impl TryFrom<PmGraphicsStyleCollectionRecordWire> for PmGraphicsStyleCollectionRecord {
    type Error = String;

    fn try_from(wire: PmGraphicsStyleCollectionRecordWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            segment_token: wire.segment_token,
            record_ordinal: wire.record_ordinal,
            segment_version_major: wire.segment_version_major,
            style_references: PmDcPairedReferenceList::new(
                wire.list_metadata,
                PmDcReference::zip(wire.style_references, wire.style_reference_qualifiers)?,
            )
            .ok_or("list_metadata disagrees with style_references")?,
        })
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UnpairedMember {
    Bulk,
    Metadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct UnpairedSegmentRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) missing_member: UnpairedMember,
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
    pub(crate) number: crate::records::MetaSectionNumber,
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
#[serde(
    try_from = "SegmentMetaIssueRecordWire",
    into = "SegmentMetaIssueRecordWire"
)]
pub(crate) struct SegmentMetaIssueRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) detail: String,
}

#[derive(Serialize, Deserialize)]
struct SegmentMetaIssueRecordWire {
    id: String,
    token: String,
    status: String,
    detail: String,
}

impl From<SegmentMetaIssueRecord> for SegmentMetaIssueRecordWire {
    fn from(value: SegmentMetaIssueRecord) -> Self {
        Self {
            id: value.id,
            token: value.token,
            status: "malformed".into(),
            detail: value.detail,
        }
    }
}

impl TryFrom<SegmentMetaIssueRecordWire> for SegmentMetaIssueRecord {
    type Error = String;

    fn try_from(wire: SegmentMetaIssueRecordWire) -> Result<Self, Self::Error> {
        if wire.status != "malformed" {
            return Err(format!("segment meta issue status {}", wire.status));
        }
        Ok(Self {
            id: wire.id,
            token: wire.token,
            detail: wire.detail,
        })
    }
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
}

#[derive(Serialize, Deserialize)]
struct SegmentBulkRecordWire {
    id: String,
    token: String,
    prefix: String,
    form: u16,
    compressed_len: u64,
    compressed_sha256: String,
    expanded_len: u64,
    expanded_sha256: String,
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
            };
        Self {
            id: value.id,
            token: value.token,
            prefix: value.prefix,
            form: value.form,
            compressed_len: value.compressed_len,
            compressed_sha256: value.compressed_sha256,
            expanded_len: value.expanded_len,
            expanded_sha256: value.expanded_sha256,
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
        let records = match wire.record_state.as_str() {
            "framed" if wire.record_detail.is_none() => SegmentBulkFrame::Framed {
                record_count: wire.record_count,
                stream_trailer_len: wire
                    .stream_trailer_len
                    .ok_or_else(|| "framed bulk requires stream_trailer_len".to_owned())?,
                stream_trailer_sha256: wire
                    .stream_trailer_sha256
                    .ok_or_else(|| "framed bulk requires stream_trailer_sha256".to_owned())?,
            },
            "framed" => return Err("framed bulk cannot carry record_detail".to_owned()),
            "unavailable"
                if wire.record_count == 0
                    && wire.stream_trailer_len.is_none()
                    && wire.stream_trailer_sha256.is_none() =>
            {
                SegmentBulkFrame::Unavailable {
                    detail: wire
                        .record_detail
                        .ok_or_else(|| "unavailable bulk requires record_detail".to_owned())?,
                }
            }
            "unavailable" => {
                return Err("unavailable bulk cannot carry framed record fields".to_owned());
            }
            other => return Err(format!("unknown bulk record_state {other}")),
        };
        Ok(Self {
            id: wire.id,
            token: wire.token,
            prefix: wire.prefix,
            form: wire.form,
            compressed_len: wire.compressed_len,
            compressed_sha256: wire.compressed_sha256,
            expanded_len: wire.expanded_len,
            expanded_sha256: wire.expanded_sha256,
            records,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RseRecordRecordWire", into = "RseRecordRecordWire")]
pub(crate) struct RseRecordRecord {
    pub(crate) id: String,
    pub(crate) token: String,
    pub(crate) ordinal: u32,
    selector: u32,
    type_index: u8,
    pub(crate) type_id: String,
    pub(crate) payload_offset: u64,
    payload_len: u64,
    pub(crate) payload_sha256: String,
    trailing_payload_len: u32,
    pub(crate) trailer_len: u64,
    pub(crate) trailer_sha256: String,
}

impl RseRecordRecord {
    pub(crate) fn from_frame(token: &str, frame: &crate::records::RseRecordFrame<'_>) -> Self {
        Self {
            id: format!("inventor:rse:record#{token}-{}", frame.ordinal),
            token: token.into(),
            ordinal: frame.ordinal,
            selector: frame.selector,
            type_index: frame.type_index(),
            type_id: crate::pmdc::type_id_string(frame.type_id),
            payload_offset: frame.payload_offset,
            payload_len: u64::from(frame.payload_len()),
            payload_sha256: cadmpeg_ir::hash::sha256_hex(frame.payload.window()),
            trailing_payload_len: frame.trailing_payload_len(),
            trailer_len: frame.trailer.window().len() as u64,
            trailer_sha256: cadmpeg_ir::hash::sha256_hex(frame.trailer.window()),
        }
    }
    pub(crate) fn type_index(&self) -> u8 {
        self.type_index
    }
    pub(crate) fn payload_len(&self) -> u64 {
        self.payload_len
    }
}

#[derive(Serialize, Deserialize)]
struct RseRecordRecordWire {
    id: String,
    token: String,
    ordinal: u32,
    selector: u32,
    type_index: u8,
    type_id: String,
    payload_offset: u64,
    payload_len: u64,
    payload_sha256: String,
    trailing_payload_len: u32,
    trailer_len: u64,
    trailer_sha256: String,
}

impl From<RseRecordRecord> for RseRecordRecordWire {
    fn from(record: RseRecordRecord) -> Self {
        Self {
            id: record.id,
            token: record.token,
            ordinal: record.ordinal,
            selector: record.selector,
            type_index: record.type_index,
            type_id: record.type_id,
            payload_offset: record.payload_offset,
            payload_len: record.payload_len,
            payload_sha256: record.payload_sha256,
            trailing_payload_len: record.trailing_payload_len,
            trailer_len: record.trailer_len,
            trailer_sha256: record.trailer_sha256,
        }
    }
}

impl TryFrom<RseRecordRecordWire> for RseRecordRecord {
    type Error = String;
    fn try_from(wire: RseRecordRecordWire) -> Result<Self, Self::Error> {
        if wire.type_index != wire.selector as u8 {
            return Err("type_index disagrees with selector".into());
        }
        if wire.trailing_payload_len != 0
            && u64::from(wire.trailing_payload_len) != wire.payload_len
        {
            return Err("trailing_payload_len disagrees with payload_len".into());
        }
        Ok(Self {
            id: wire.id,
            token: wire.token,
            ordinal: wire.ordinal,
            selector: wire.selector,
            type_index: wire.type_index,
            type_id: wire.type_id,
            payload_offset: wire.payload_offset,
            payload_len: wire.payload_len,
            payload_sha256: wire.payload_sha256,
            trailing_payload_len: wire.trailing_payload_len,
            trailer_len: wire.trailer_len,
            trailer_sha256: wire.trailer_sha256,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ActiveCarrierRecordWire", into = "ActiveCarrierRecordWire")]
pub(crate) enum ActiveCarrierRecord {
    NotApplicable {
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
        let has_selected_fields = wire.segment_token.is_some()
            || wire.record_ordinal.is_some()
            || wire.segment_version_major.is_some()
            || wire.family.is_some()
            || wire.header_state.is_some()
            || wire.header_kind.is_some()
            || wire.header_value.is_some()
            || wire.schema.is_some()
            || wire.carrier_len.is_some()
            || wire.carrier_offset.is_some()
            || wire.carrier_sha256.is_some()
            || wire.selected_key.is_some()
            || wire.enabled.is_some()
            || wire.delta_state.is_some()
            || wire.history_reference.is_some();
        if !matches!(wire.state, ActiveCarrierRecordState::Selected) && has_selected_fields {
            return Err("inactive carrier cannot carry selected fields".to_owned());
        }
        if !matches!(wire.state, ActiveCarrierRecordState::Unavailable) && wire.detail.is_some() {
            return Err("active carrier detail requires unavailable state".to_owned());
        }
        match wire.state {
            ActiveCarrierRecordState::NotApplicable => Ok(Self::NotApplicable { id: wire.id }),
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
                        ));
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

#[cfg(test)]
mod tests {
    use super::{
        ActiveCarrierRecord, PmAppRenderingStyleRecord, SegmentBulkFrame, SegmentBulkRecord,
    };

    #[test]
    fn bulk_wire_requires_expansion_and_preserves_exclusive_frame_fields() {
        let record = SegmentBulkRecord {
            id: "inventor:rse:bulk#1".into(),
            token: "segment".into(),
            prefix: "RSe".into(),
            form: 1,
            compressed_len: 0,
            compressed_sha256: cadmpeg_ir::hash::sha256_hex(&[]),
            expanded_len: 0,
            expanded_sha256: cadmpeg_ir::hash::sha256_hex(&[]),
            records: SegmentBulkFrame::Framed {
                record_count: 0,
                stream_trailer_len: 0,
                stream_trailer_sha256: cadmpeg_ir::hash::sha256_hex(&[]),
            },
        };
        let wire = serde_json::to_value(&record).expect("valid test fixture");
        assert_eq!(
            serde_json::from_value::<SegmentBulkRecord>(wire.clone()).expect("valid test fixture"),
            record
        );
        for field in [
            "expanded_len",
            "expanded_sha256",
            "stream_trailer_len",
            "stream_trailer_sha256",
        ] {
            let mut missing = wire.clone();
            missing
                .as_object_mut()
                .expect("valid test fixture")
                .remove(field);
            assert!(
                serde_json::from_value::<SegmentBulkRecord>(missing).is_err(),
                "{field}"
            );
        }
        let mut mixed = wire.clone();
        mixed["record_detail"] = serde_json::json!("unavailable");
        assert!(serde_json::from_value::<SegmentBulkRecord>(mixed).is_err());
        for state in ["not_expanded", "future"] {
            let mut invalid = wire.clone();
            invalid["record_state"] = serde_json::json!(state);
            assert!(serde_json::from_value::<SegmentBulkRecord>(invalid).is_err());
        }
        let unavailable = SegmentBulkRecord {
            records: SegmentBulkFrame::Unavailable {
                detail: "no frame".into(),
            },
            ..record
        };
        let mut wire = serde_json::to_value(&unavailable).expect("valid test fixture");
        assert_eq!(
            serde_json::from_value::<SegmentBulkRecord>(wire.clone()).expect("valid test fixture"),
            unavailable
        );
        wire["record_count"] = serde_json::json!(1);
        assert!(serde_json::from_value::<SegmentBulkRecord>(wire).is_err());
    }

    #[test]
    fn inactive_carrier_wire_rejects_selected_payload_and_unknown_states() {
        let record = ActiveCarrierRecord::NotApplicable {
            id: "inventor:rse:active-carrier#1".into(),
        };
        let wire = serde_json::to_value(&record).expect("valid test fixture");
        assert_eq!(
            serde_json::from_value::<ActiveCarrierRecord>(wire.clone())
                .expect("valid test fixture"),
            record
        );
        for (field, value) in [
            ("state", serde_json::json!("not_expanded")),
            ("state", serde_json::json!("future")),
            ("carrier_len", serde_json::json!(1)),
            ("detail", serde_json::json!("unavailable")),
        ] {
            let mut invalid = wire.clone();
            invalid[field] = value;
            assert!(
                serde_json::from_value::<ActiveCarrierRecord>(invalid).is_err(),
                "{field}"
            );
        }
        let unavailable = ActiveCarrierRecord::Unavailable {
            id: "inventor:rse:active-carrier#1".into(),
            detail: "no selection".into(),
        };
        let mut wire = serde_json::to_value(&unavailable).expect("valid test fixture");
        assert_eq!(
            serde_json::from_value::<ActiveCarrierRecord>(wire.clone())
                .expect("valid test fixture"),
            unavailable
        );
        wire["segment_token"] = serde_json::json!("segment");
        assert!(serde_json::from_value::<ActiveCarrierRecord>(wire).is_err());
    }
    #[test]
    fn rendering_style_version_controls_comment_and_extension() {
        let legacy = serde_json::json!({
            "id": "style", "segment_token": "segment", "record_ordinal": 0,
            "segment_version_major": 16, "header_value": 0, "header_id": 0,
            "state": 0, "flags": 0, "values": [0, 0], "default_state": 0,
            "value": 0, "name_reference": 0, "name": "", "comment": "comment",
            "long_name": "", "suffix_len": 0, "suffix_sha256": ""
        });
        assert!(serde_json::from_value::<PmAppRenderingStyleRecord>(legacy.clone()).is_ok());
        let mut modern = legacy.clone();
        modern["segment_version_major"] = serde_json::json!(17);
        modern["comment"] = serde_json::json!("");
        assert!(serde_json::from_value::<PmAppRenderingStyleRecord>(modern.clone()).is_err());
        for (field, value) in [
            ("style_state", serde_json::json!(0)),
            ("style_label", serde_json::json!("")),
            ("asset_guid", serde_json::json!("")),
            ("material_id", serde_json::json!("")),
            ("asset_library_id", serde_json::json!("")),
            ("style_values", serde_json::json!([0, 0])),
            ("guid", serde_json::json!("")),
        ] {
            modern[field] = value;
        }
        assert!(serde_json::from_value::<PmAppRenderingStyleRecord>(modern.clone()).is_ok());
        let mut invalid = modern.clone();
        invalid["comment"] = serde_json::json!("comment");
        assert!(serde_json::from_value::<PmAppRenderingStyleRecord>(invalid).is_err());
        modern["segment_version_major"] = serde_json::json!(16);
        assert!(serde_json::from_value::<PmAppRenderingStyleRecord>(modern).is_err());
    }
    #[test]
    fn property_kind_wire_agrees_with_ole_type_code() {
        for (code, kind) in [
            (0, "empty"),
            (1, "empty"),
            (2, "signed"),
            (3, "signed"),
            (4, "float"),
            (5, "float"),
            (6, "signed"),
            (7, "float"),
            (8, "string"),
            (10, "signed"),
            (11, "bool"),
            (16, "signed"),
            (17, "unsigned"),
            (18, "unsigned"),
            (19, "unsigned"),
            (20, "signed"),
            (21, "unsigned"),
            (22, "signed"),
            (23, "unsigned"),
            (30, "string"),
            (31, "string"),
            (64, "filetime"),
            (65, "binary:8"),
            (70, "binary:0"),
            (71, "clipboard:3:8"),
            (72, "guid"),
            (0x100c, "vector:3"),
            (0x1003, "vector:0"),
            (0x999, "unknown"),
        ] {
            let value = super::PropertyValueKind::from_wire(kind, Some(code))
                .expect("kind matches the OLE type code");
            assert_eq!(value.to_string(), kind);
            assert_eq!(value.type_code(), Some(code));
            let wrong = if kind == "signed" {
                "unsigned"
            } else {
                "signed"
            };
            assert!(super::PropertyValueKind::from_wire(wrong, Some(code)).is_err());
        }
        assert!(super::PropertyValueKind::from_wire("unknown", Some(3)).is_err());
        assert!(super::PropertyValueKind::from_wire("dictionary", Some(0)).is_err());
        assert!(super::PropertyValueKind::from_wire("dictionary", None).is_ok());
    }
    #[test]
    fn assembly_matrix_wire_preserves_keys_and_rejects_mask_disagreement() {
        let mut wire = serde_json::json!({
            "id": "placement", "segment_token": "segment", "record_ordinal": 0,
            "header_id": 0, "owner_reference": 0, "attribute_reference": 0,
            "state": 0, "transform_prefix": false, "transform_encoding": [33825, 31710],
            "transform": [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0],
                          [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
            "branch": 0, "graphics_state": 0, "occurrence_id": 0,
            "graphics_index": 0, "object_reference": 0, "suffix_len": 0, "suffix_sha256": ""
        });
        let placement: super::AssemblyPlacementRecord = serde_json::from_value(wire.clone())
            .expect("assembly matrix fixture agrees with its masks");
        assert_eq!(
            serde_json::to_value(placement).expect("assembly matrix fixture agrees with its masks"),
            wire
        );
        wire["transform"][0][0] = serde_json::json!(2.0);
        assert!(serde_json::from_value::<super::AssemblyPlacementRecord>(wire.clone()).is_err());
        wire["transform_encoding"] = serde_json::json!([0, 0]);
        assert!(serde_json::from_value::<super::AssemblyPlacementRecord>(wire).is_ok());
    }
}
