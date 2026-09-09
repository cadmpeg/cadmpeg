// SPDX-License-Identifier: Apache-2.0
//! `UFRx` document states and their owned child records.

use crate::native::digest::Sha256Hex;
use cadmpeg_ir::products::NonEmptyString;

use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::{de::Error as _, Deserialize, Serialize};
use std::num::NonZeroU64;

#[derive(Debug, Clone, PartialEq, Eq)]
// One document state owns all child arenas; no extra box is needed for the singleton header.
#[allow(clippy::large_enum_variant)]
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
        model_states: Vec<UfrxModelStateRecord>,
        external_references: Vec<ExternalReferenceRecord>,
        embedded_references: Vec<EmbeddedReferenceRecord>,
        occurrences: Vec<UfrxOccurrenceRecord>,
        tail_len: u64,
        tail_sha256: Sha256Hex,
    },
    Unsupported {
        id: String,
        directory_id: u32,
        schema: u16,
        section_versions: Vec<u16>,
        tail_len: u64,
        tail_sha256: Sha256Hex,
        detail: String,
    },
    Malformed {
        id: String,
        directory_id: u32,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "UfrxRepresentationRecordWire",
    into = "UfrxRepresentationRecordWire"
)]
pub(crate) struct UfrxRepresentationRecord {
    pub(crate) prefix: u16,
    pub(crate) active_representation: Option<(NonEmptyString, NonEmptyString)>,
    pub(crate) secondary_active_lod_state: [u16; 2],
    pub(crate) active_model_state: NonEmptyString,
    pub(crate) active_model_state_state: [u16; 2],
}

#[derive(Serialize, Deserialize)]
pub(crate) struct UfrxRepresentationRecordWire {
    pub(crate) prefix: u16,
    pub(crate) active_representation: Option<String>,
    pub(crate) active_representation_kind: Option<String>,
    pub(crate) secondary_active_lod_state: [u16; 2],
    pub(crate) active_model_state: String,
    pub(crate) active_model_state_state: [u16; 2],
}

impl From<UfrxRepresentationRecord> for UfrxRepresentationRecordWire {
    fn from(value: UfrxRepresentationRecord) -> Self {
        let (active_representation, active_representation_kind) = value
            .active_representation
            .map_or((None, None), |(name, kind)| {
                (
                    Some(name.as_str().to_owned()),
                    Some(kind.as_str().to_owned()),
                )
            });
        Self {
            prefix: value.prefix,
            active_representation,
            active_representation_kind,
            secondary_active_lod_state: value.secondary_active_lod_state,
            active_model_state: value.active_model_state.as_str().to_owned(),
            active_model_state_state: value.active_model_state_state,
        }
    }
}

impl TryFrom<UfrxRepresentationRecordWire> for UfrxRepresentationRecord {
    type Error = String;
    fn try_from(wire: UfrxRepresentationRecordWire) -> Result<Self, Self::Error> {
        let active_representation =
            match (wire.active_representation, wire.active_representation_kind) {
                (None, None) => None,
                (Some(name), Some(kind)) => Some((
                    NonEmptyString::new(name).ok_or("active_representation must not be empty")?,
                    NonEmptyString::new(kind)
                        .ok_or("active_representation_kind must not be empty")?,
                )),
                _ => return Err(
                    "active_representation and active_representation_kind must be present together"
                        .into(),
                ),
            };
        Ok(Self {
            prefix: wire.prefix,
            active_representation,
            secondary_active_lod_state: wire.secondary_active_lod_state,
            active_model_state: NonEmptyString::new(wire.active_model_state)
                .ok_or("active_model_state must not be empty")?,
            active_model_state_state: wire.active_model_state_state,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "UfrxModelStateRecordWire",
    into = "UfrxModelStateRecordWire"
)]
pub(crate) struct UfrxModelStateRecord {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) prefix: u8,
    name: NonEmptyString,
    pub(crate) state: [u16; 2],
    pub(crate) prefix_count: u32,
    pub(crate) parameters: Vec<UfrxModelStateParameterRecord>,
    suffix_sha256: Sha256Hex,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct UfrxModelStateRecordWire {
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

impl TryFrom<UfrxModelStateRecordWire> for UfrxModelStateRecord {
    type Error = String;
    fn try_from(wire: UfrxModelStateRecordWire) -> Result<Self, Self::Error> {
        if wire.suffix_len != 77 {
            return Err("suffix_len must be 77".into());
        }
        Ok(Self {
            id: wire.id,
            ordinal: wire.ordinal,
            prefix: wire.prefix,
            name: NonEmptyString::new(wire.name).ok_or("name must not be empty")?,
            state: wire.state,
            prefix_count: wire.prefix_count,
            parameters: wire.parameters,
            suffix_sha256: Sha256Hex::try_from(wire.suffix_sha256)
                .map_err(|error| format!("suffix_sha256: {error}"))?,
        })
    }
}

impl From<UfrxModelStateRecord> for UfrxModelStateRecordWire {
    fn from(value: UfrxModelStateRecord) -> Self {
        Self {
            id: value.id,
            ordinal: value.ordinal,
            prefix: value.prefix,
            name: value.name.as_str().to_owned(),
            state: value.state,
            prefix_count: value.prefix_count,
            parameters: value.parameters,
            suffix_sha256: value.suffix_sha256.into(),
            suffix_len: 77,
        }
    }
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

impl From<&UfrxRecord> for UfrxRecordWire {
    fn from(value: &UfrxRecord) -> Self {
        match value {
            UfrxRecord::Absent { id } => Self {
                id: id.clone(),
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
                model_states,
                external_references,
                embedded_references,
                occurrences,
                tail_len,
                tail_sha256,
            } => Self {
                id: id.clone(),
                state: UfrxRecordState::ParsedPrefix,
                directory_id: Some(*directory_id),
                schema: Some(*schema),
                section_versions: section_versions.clone(),
                original_file_name: Some(original_file_name.clone()),
                caption: Some(caption.clone()),
                representation: representation.clone(),
                model_state_count: model_states.len() as u64,
                reference_count: external_references.len() as u64,
                embedded_reference_count: embedded_references.len() as u64,
                occurrence_count: occurrences.len() as u64,
                tail_len: *tail_len,
                tail_sha256: Some(tail_sha256.clone().into()),
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
                id: id.clone(),
                state: UfrxRecordState::Unsupported,
                directory_id: Some(*directory_id),
                schema: Some(*schema),
                section_versions: section_versions.clone(),
                original_file_name: None,
                caption: None,
                representation: None,
                model_state_count: 0,
                reference_count: 0,
                embedded_reference_count: 0,
                occurrence_count: 0,
                tail_len: *tail_len,
                tail_sha256: Some(tail_sha256.clone().into()),
                detail: Some(detail.clone()),
            },
            UfrxRecord::Malformed {
                id,
                directory_id,
                detail,
            } => Self {
                id: id.clone(),
                state: UfrxRecordState::Malformed,
                directory_id: Some(*directory_id),
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
                detail: Some(detail.clone()),
            },
        }
    }
}

impl UfrxRecordWire {
    fn into_record(
        self,
        model_states: Vec<UfrxModelStateRecord>,
        external_references: Vec<ExternalReferenceRecord>,
        embedded_references: Vec<EmbeddedReferenceRecord>,
        occurrences: Vec<UfrxOccurrenceRecord>,
    ) -> Result<UfrxRecord, String> {
        let wire = self;
        if wire.model_state_count != model_states.len() as u64 {
            return Err("UFRx model_state_count does not match its arena".into());
        }
        if wire.reference_count != external_references.len() as u64 {
            return Err("UFRx reference_count does not match its arena".into());
        }
        if wire.embedded_reference_count != embedded_references.len() as u64 {
            return Err("UFRx embedded_reference_count does not match its arena".into());
        }
        if wire.occurrence_count != occurrences.len() as u64 {
            return Err("UFRx occurrence_count does not match its arena".into());
        }
        let has_children = !model_states.is_empty()
            || !external_references.is_empty()
            || !embedded_references.is_empty()
            || !occurrences.is_empty();
        let has_parsed_fields = wire.original_file_name.is_some()
            || wire.caption.is_some()
            || wire.representation.is_some()
            || has_children;
        let has_tail = wire.tail_len != 0 || wire.tail_sha256.is_some();
        let invalid = match wire.state {
            UfrxRecordState::Absent => {
                wire.directory_id.is_some()
                    || wire.schema.is_some()
                    || !wire.section_versions.is_empty()
                    || has_parsed_fields
                    || has_tail
                    || wire.detail.is_some()
            }
            UfrxRecordState::Malformed => {
                wire.schema.is_some()
                    || !wire.section_versions.is_empty()
                    || has_parsed_fields
                    || has_tail
            }
            UfrxRecordState::Unsupported => has_parsed_fields,
            UfrxRecordState::ParsedPrefix => wire.detail.is_some(),
        };
        if invalid {
            return Err("UFRx state carries incompatible fields".into());
        }
        match wire.state {
            UfrxRecordState::Absent => Ok(UfrxRecord::Absent { id: wire.id }),
            UfrxRecordState::ParsedPrefix => Ok(UfrxRecord::ParsedPrefix {
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
                model_states,
                external_references,
                embedded_references,
                occurrences,
                tail_len: wire.tail_len,
                tail_sha256: Sha256Hex::try_from(
                    wire.tail_sha256
                        .ok_or_else(|| "parsed UFRxDoc requires tail_sha256".to_owned())?,
                )
                .map_err(|detail| format!("tail_sha256: {detail}"))?,
            }),
            UfrxRecordState::Unsupported => Ok(UfrxRecord::Unsupported {
                id: wire.id,
                directory_id: wire
                    .directory_id
                    .ok_or_else(|| "unsupported UFRxDoc requires directory_id".to_owned())?,
                schema: wire
                    .schema
                    .ok_or_else(|| "unsupported UFRxDoc requires schema".to_owned())?,
                section_versions: wire.section_versions,
                tail_len: wire.tail_len,
                tail_sha256: Sha256Hex::try_from(
                    wire.tail_sha256
                        .ok_or_else(|| "unsupported UFRxDoc requires tail_sha256".to_owned())?,
                )
                .map_err(|detail| format!("tail_sha256: {detail}"))?,
                detail: wire
                    .detail
                    .ok_or_else(|| "unsupported UFRxDoc requires detail".to_owned())?,
            }),
            UfrxRecordState::Malformed => Ok(UfrxRecord::Malformed {
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
#[serde(
    try_from = "ExternalReferenceRecordWire",
    into = "ExternalReferenceRecordWire"
)]
pub(crate) struct ExternalReferenceRecord {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    identity: ExternalReferenceIdentity,
    pub(crate) library_id: i32,
    pub(crate) library_name: String,
    pub(crate) display_name: String,
    pub(crate) state_groups: Vec<[u16; 3]>,
    pub(crate) state: [u16; 2],
    pub(crate) database_id: String,
    pub(crate) reference_id: u32,
    pub(crate) occurrence_count: u32,
    pub(crate) version: u32,
    pub(crate) flags: u32,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ExternalReferenceRecordWire {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) path: String,
    pub(crate) library_id: i32,
    pub(crate) library_name: String,
    pub(crate) display_name: String,
    pub(crate) state_groups: Vec<[u16; 3]>,
    pub(crate) state: [u16; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) document_id: Option<String>,
    pub(crate) database_id: String,
    pub(crate) reference_id: u32,
    pub(crate) occurrence_count: u32,
    pub(crate) version: u32,
    pub(crate) flags: u32,
}

impl TryFrom<ExternalReferenceRecordWire> for ExternalReferenceRecord {
    type Error = String;
    fn try_from(wire: ExternalReferenceRecordWire) -> Result<Self, Self::Error> {
        let document_id = wire
            .document_id
            .filter(|value| !value.chars().all(|character| character == '0'))
            .and_then(NonEmptyString::new);
        Ok(Self {
            id: wire.id,
            ordinal: wire.ordinal,
            identity: match NonEmptyString::new(wire.path) {
                Some(path) => ExternalReferenceIdentity::Path { path, document_id },
                None => ExternalReferenceIdentity::DocumentId(
                    document_id.ok_or("path or a nonzero document_id is required")?,
                ),
            },
            library_id: wire.library_id,
            library_name: wire.library_name,
            display_name: wire.display_name,
            state_groups: wire.state_groups,
            state: wire.state,
            database_id: wire.database_id,
            reference_id: wire.reference_id,
            occurrence_count: wire.occurrence_count,
            version: wire.version,
            flags: wire.flags,
        })
    }
}

impl From<ExternalReferenceRecord> for ExternalReferenceRecordWire {
    fn from(value: ExternalReferenceRecord) -> Self {
        let (path, document_id) = match value.identity {
            ExternalReferenceIdentity::Path { path, document_id } => (
                path.as_str().to_owned(),
                document_id.map(|value| value.as_str().to_owned()),
            ),
            ExternalReferenceIdentity::DocumentId(document_id) => {
                (String::new(), Some(document_id.as_str().to_owned()))
            }
        };
        Self {
            id: value.id,
            ordinal: value.ordinal,
            path,
            library_id: value.library_id,
            library_name: value.library_name,
            display_name: value.display_name,
            state_groups: value.state_groups,
            state: value.state,
            document_id,
            database_id: value.database_id,
            reference_id: value.reference_id,
            occurrence_count: value.occurrence_count,
            version: value.version,
            flags: value.flags,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExternalReferenceIdentity {
    Path {
        path: NonEmptyString,
        document_id: Option<NonEmptyString>,
    },
    DocumentId(NonEmptyString),
}

impl ExternalReferenceRecord {
    pub(crate) fn document(&self) -> cadmpeg_ir::products::ExternalDocumentReference {
        use cadmpeg_ir::products::ExternalDocumentReference;
        match &self.identity {
            ExternalReferenceIdentity::Path { path, .. } => {
                ExternalDocumentReference::Path(path.clone())
            }
            ExternalReferenceIdentity::DocumentId(document_id) => {
                ExternalDocumentReference::DocumentId(document_id.clone())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "EmbeddedReferenceRecordWire",
    into = "EmbeddedReferenceRecordWire"
)]
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
    record_len: NonZeroU64,
    record_sha256: Sha256Hex,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct EmbeddedReferenceRecordWire {
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

impl TryFrom<EmbeddedReferenceRecordWire> for EmbeddedReferenceRecord {
    type Error = String;
    fn try_from(wire: EmbeddedReferenceRecordWire) -> Result<Self, Self::Error> {
        Ok(Self {
            id: wire.id,
            ordinal: wire.ordinal,
            value_0: wire.value_0,
            filetime: wire.filetime,
            value_1: wire.value_1,
            extended_value: wire.extended_value,
            value_2: wire.value_2,
            path: wire.path,
            library_id: wire.library_id,
            library_name: wire.library_name,
            state: wire.state,
            display_name: wire.display_name,
            state_values: wire.state_values,
            record_len: NonZeroU64::new(wire.record_len).ok_or("record_len must not be zero")?,
            record_sha256: Sha256Hex::try_from(wire.record_sha256)
                .map_err(|error| format!("record_sha256: {error}"))?,
        })
    }
}

impl From<EmbeddedReferenceRecord> for EmbeddedReferenceRecordWire {
    fn from(value: EmbeddedReferenceRecord) -> Self {
        Self {
            id: value.id,
            ordinal: value.ordinal,
            value_0: value.value_0,
            filetime: value.filetime,
            value_1: value.value_1,
            extended_value: value.extended_value,
            value_2: value.value_2,
            path: value.path,
            library_id: value.library_id,
            library_name: value.library_name,
            state: value.state,
            display_name: value.display_name,
            state_values: value.state_values,
            record_len: value.record_len.get(),
            record_sha256: value.record_sha256.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "UfrxOccurrenceRecordWire",
    into = "UfrxOccurrenceRecordWire"
)]
pub(crate) struct UfrxOccurrenceRecord {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) end_string_flag: u32,
    pub(crate) file_reference_id: u32,
    pub(crate) occurrence_id: u32,
    pub(crate) header_value: u32,
    pub(crate) title: Option<String>,
    header_padding_words: u8,
    record_len: NonZeroU64,
    record_sha256: Sha256Hex,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct UfrxOccurrenceRecordWire {
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

impl TryFrom<UfrxOccurrenceRecordWire> for UfrxOccurrenceRecord {
    type Error = String;
    fn try_from(wire: UfrxOccurrenceRecordWire) -> Result<Self, Self::Error> {
        if wire.header_padding_words > 8 {
            return Err("header_padding_words must not exceed 8".into());
        }
        Ok(Self {
            id: wire.id,
            ordinal: wire.ordinal,
            end_string_flag: wire.end_string_flag,
            file_reference_id: wire.file_reference_id,
            occurrence_id: wire.occurrence_id,
            header_value: wire.header_value,
            title: wire.title,
            header_padding_words: wire.header_padding_words,
            record_len: NonZeroU64::new(wire.record_len).ok_or("record_len must not be zero")?,
            record_sha256: Sha256Hex::try_from(wire.record_sha256)
                .map_err(|error| format!("record_sha256: {error}"))?,
        })
    }
}

impl From<UfrxOccurrenceRecord> for UfrxOccurrenceRecordWire {
    fn from(value: UfrxOccurrenceRecord) -> Self {
        Self {
            id: value.id,
            ordinal: value.ordinal,
            end_string_flag: value.end_string_flag,
            file_reference_id: value.file_reference_id,
            occurrence_id: value.occurrence_id,
            header_value: value.header_value,
            title: value.title,
            header_padding_words: value.header_padding_words,
            record_len: value.record_len.get(),
            record_sha256: value.record_sha256.into(),
        }
    }
}

impl Serialize for UfrxRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        UfrxRecordWire::from(self).serialize(serializer)
    }
}
impl UfrxRecord {
    pub(crate) fn model_states(&self) -> &[UfrxModelStateRecord] {
        match self {
            Self::ParsedPrefix { model_states, .. } => model_states,
            _ => &[],
        }
    }
    pub(crate) fn external_references(&self) -> &[ExternalReferenceRecord] {
        match self {
            Self::ParsedPrefix {
                external_references,
                ..
            } => external_references,
            _ => &[],
        }
    }
    pub(crate) fn embedded_references(&self) -> &[EmbeddedReferenceRecord] {
        match self {
            Self::ParsedPrefix {
                embedded_references,
                ..
            } => embedded_references,
            _ => &[],
        }
    }
    pub(crate) fn occurrences(&self) -> &[UfrxOccurrenceRecord] {
        match self {
            Self::ParsedPrefix { occurrences, .. } => occurrences,
            _ => &[],
        }
    }
    pub(crate) fn install(
        &self,
        namespace: &mut NativeNamespace,
    ) -> Result<(), NativeConvertError> {
        namespace.set_arena("ufrx", std::slice::from_ref(self))?;
        namespace.set_arena("ufrx_model_states", self.model_states())?;
        namespace.set_arena("external_references", self.external_references())?;
        namespace.set_arena("embedded_references", self.embedded_references())?;
        namespace.set_arena("ufrx_occurrences", self.occurrences())?;
        Ok(())
    }
    pub(crate) fn read(namespace: &NativeNamespace) -> Result<Self, NativeConvertError> {
        let [wire] = <[_; 1]>::try_from(namespace.arena_as::<UfrxRecordWire>("ufrx")?).map_err(
            |records: Vec<_>| {
                serde_json::Error::custom(format!(
                    "Inventor native data has {} UFRxDoc state records",
                    records.len()
                ))
            },
        )?;
        wire.into_record(
            namespace.arena_as("ufrx_model_states")?,
            namespace.arena_as("external_references")?,
            namespace.arena_as("embedded_references")?,
            namespace.arena_as("ufrx_occurrences")?,
        )
        .map_err(|detail| serde_json::Error::custom(detail).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_reference_requires_path_or_nonzero_document_id() {
        let valid = serde_json::json!({
            "id": "reference", "ordinal": 0, "path": "part.ipt", "library_id": 0,
            "library_name": "", "display_name": "", "state_groups": [], "state": [0,0],
            "document_id": "0".repeat(32), "database_id": "", "reference_id": 1,
            "occurrence_count": 0, "version": 0, "flags": 0
        });
        for (path, document_id, accepted) in [
            ("part.ipt", "0000", true),
            ("part.ipt", "", true),
            ("", "0001", true),
            ("part.ipt", "0001", true),
            ("", "0000", false),
            ("", "", false),
        ] {
            let mut wire = valid.clone();
            wire["path"] = serde_json::json!(path);
            wire["document_id"] = serde_json::json!(document_id);
            let record = serde_json::from_value::<ExternalReferenceRecord>(wire.clone());
            if accepted {
                if document_id.chars().all(|character| character == '0') {
                    wire.as_object_mut().unwrap().remove("document_id");
                }
                assert_eq!(
                    serde_json::to_value(record.expect("valid native record fixture"))
                        .expect("valid native record fixture"),
                    wire
                );
            } else {
                assert!(record
                    .expect_err("invalid native record fixture")
                    .to_string()
                    .contains("document_id"));
            }
        }
    }

    #[test]
    fn reference_framing_admission_rejects_invalid_wire_values() {
        let occurrence = serde_json::json!({
            "id": "occurrence", "ordinal": 0, "end_string_flag": 0,
            "file_reference_id": 1, "occurrence_id": 1, "header_value": 0,
            "title": null, "header_padding_words": 8, "record_len": 1,
            "record_sha256": "A".repeat(64)
        });
        let embedded = serde_json::json!({
            "id": "embedded", "ordinal": 0, "value_0": 0, "filetime": 0,
            "value_1": 0, "extended_value": null, "value_2": 0, "path": "",
            "library_id": 0, "library_name": "", "state": 0, "display_name": "",
            "state_values": [0,0,0,0,0,0,0,0], "record_len": 1,
            "record_sha256": "a".repeat(64)
        });
        let admitted: UfrxOccurrenceRecord =
            serde_json::from_value(occurrence.clone()).expect("valid native record fixture");
        assert_eq!(
            serde_json::to_value(admitted).expect("valid native record fixture"),
            occurrence
        );
        let admitted: EmbeddedReferenceRecord =
            serde_json::from_value(embedded.clone()).expect("valid native record fixture");
        assert_eq!(
            serde_json::to_value(admitted).expect("valid native record fixture"),
            embedded
        );
        for (field, value) in [
            ("record_len", serde_json::json!(0)),
            ("record_sha256", serde_json::json!("a".repeat(63))),
            ("record_sha256", serde_json::json!("g".repeat(64))),
        ] {
            let mut wire = occurrence.clone();
            wire[field] = value.clone();
            assert!(serde_json::from_value::<UfrxOccurrenceRecord>(wire)
                .expect_err("invalid native record fixture")
                .to_string()
                .contains(field));
            let mut wire = embedded.clone();
            wire[field] = value;
            assert!(serde_json::from_value::<EmbeddedReferenceRecord>(wire)
                .expect_err("invalid native record fixture")
                .to_string()
                .contains(field));
        }
        let mut wire = occurrence;
        wire["header_padding_words"] = serde_json::json!(9);
        assert!(serde_json::from_value::<UfrxOccurrenceRecord>(wire)
            .expect_err("invalid native record fixture")
            .to_string()
            .contains("header_padding_words"));
    }

    #[test]
    fn representation_admission_rejects_empty_names_and_half_pairs() {
        let valid = serde_json::json!({
            "prefix": 0, "active_representation": "Master",
            "active_representation_kind": "LOD", "secondary_active_lod_state": [0, 0],
            "active_model_state": "Primary", "active_model_state_state": [0, 0]
        });
        let record: UfrxRepresentationRecord =
            serde_json::from_value(valid.clone()).expect("valid native record fixture");
        assert_eq!(
            serde_json::to_value(record).expect("valid native record fixture"),
            valid
        );
        for field in [
            "active_representation",
            "active_representation_kind",
            "active_model_state",
        ] {
            let mut wire = valid.clone();
            wire[field] = serde_json::json!("");
            assert!(serde_json::from_value::<UfrxRepresentationRecord>(wire)
                .expect_err("invalid native record fixture")
                .to_string()
                .contains(field));
        }
        for field in ["active_representation", "active_representation_kind"] {
            let mut wire = valid.clone();
            wire[field] = serde_json::Value::Null;
            assert!(serde_json::from_value::<UfrxRepresentationRecord>(wire).is_err());
        }
        let mut wire = valid;
        wire["active_representation"] = serde_json::Value::Null;
        wire["active_representation_kind"] = serde_json::Value::Null;
        assert!(serde_json::from_value::<UfrxRepresentationRecord>(wire).is_ok());
    }

    #[test]
    fn model_state_admission_rejects_invalid_framing() {
        let valid = serde_json::json!({
            "id": "state", "ordinal": 0, "prefix": 0, "name": "Primary",
            "state": [0, 0], "prefix_count": 0, "parameters": [],
            "suffix_len": 77, "suffix_sha256": "a".repeat(64)
        });
        let record: UfrxModelStateRecord =
            serde_json::from_value(valid.clone()).expect("valid native record fixture");
        assert_eq!(
            serde_json::to_value(record).expect("valid native record fixture"),
            valid
        );
        for (field, value) in [
            ("name", serde_json::json!("")),
            ("suffix_len", serde_json::json!(76)),
            ("suffix_len", serde_json::json!(78)),
            ("suffix_sha256", serde_json::json!("a".repeat(63))),
            ("suffix_sha256", serde_json::json!("g".repeat(64))),
        ] {
            let mut wire = valid.clone();
            wire[field] = value;
            assert!(
                serde_json::from_value::<UfrxModelStateRecord>(wire).is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn parsed_state_owns_arenas_and_checks_wire_counts() {
        let record = UfrxRecord::ParsedPrefix {
            id: "inventor:ufrx:state#root".into(),
            directory_id: 3,
            schema: 1,
            section_versions: vec![1],
            original_file_name: "part.ipt".into(),
            caption: "part".into(),
            representation: None,
            model_states: vec![UfrxModelStateRecord::try_from(UfrxModelStateRecordWire {
                id: "inventor:ufrx:model-state#0".into(),
                ordinal: 0,
                prefix: 0,
                name: "Primary".into(),
                state: [0, 0],
                prefix_count: 0,
                parameters: vec![],
                suffix_len: 77,
                suffix_sha256: "0".repeat(64),
            })
            .expect("valid native record fixture")],
            external_references: vec![],
            embedded_references: vec![],
            occurrences: vec![],
            tail_len: 0,
            tail_sha256: Sha256Hex::try_from("0".repeat(64)).expect("64 hexadecimal digits"),
        };
        let mut namespace = NativeNamespace::default();
        record.install(&mut namespace).expect("valid test fixture");
        assert_eq!(
            UfrxRecord::read(&namespace).expect("valid test fixture"),
            record
        );
        let mut wire = namespace
            .arena_as::<serde_json::Value>("ufrx")
            .expect("valid test fixture");
        assert_eq!(wire[0]["model_state_count"], 1);
        wire[0]["model_state_count"] = serde_json::json!(0);
        namespace
            .set_arena("ufrx", &wire)
            .expect("valid test fixture");
        assert!(UfrxRecord::read(&namespace)
            .expect_err("invalid test fixture")
            .to_string()
            .contains("model_state_count"));
        let absent = UfrxRecord::Absent {
            id: "inventor:ufrx:state#root".into(),
        };
        namespace
            .set_arena("ufrx", &[absent])
            .expect("valid test fixture");
        assert!(UfrxRecord::read(&namespace).is_err());
    }

    #[test]
    fn nonparsed_states_reject_incompatible_fields() {
        for record in [
            UfrxRecord::Absent {
                id: "inventor:ufrx:state#root".into(),
            },
            UfrxRecord::Malformed {
                id: "inventor:ufrx:state#root".into(),
                directory_id: 3,
                detail: "truncated".into(),
            },
            UfrxRecord::Unsupported {
                id: "inventor:ufrx:state#root".into(),
                directory_id: 3,
                schema: 1,
                section_versions: vec![1],
                tail_len: 0,
                tail_sha256: Sha256Hex::try_from("0".repeat(64)).expect("64 hexadecimal digits"),
                detail: "schema".into(),
            },
        ] {
            let mut namespace = NativeNamespace::default();
            record.install(&mut namespace).expect("valid test fixture");
            assert_eq!(
                UfrxRecord::read(&namespace).expect("valid test fixture"),
                record
            );
            let mut wire = namespace
                .arena_as::<serde_json::Value>("ufrx")
                .expect("valid test fixture");
            wire[0]["caption"] = serde_json::json!("orphan");
            namespace
                .set_arena("ufrx", &wire)
                .expect("valid test fixture");
            assert!(UfrxRecord::read(&namespace).is_err());
        }
    }
}
