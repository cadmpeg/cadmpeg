// SPDX-License-Identifier: Apache-2.0
//! `UFRx` document states and their owned child records.

use crate::native::digest::Sha256Hex;
use cadmpeg_ir::products::NonEmptyString;

use cadmpeg_ir::native::{NativeConvertError, NativeNamespace};
use serde::{de::Error as _, Deserialize, Serialize};

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
#[serde(
    try_from = "UfrxRepresentationRecordWire",
    into = "UfrxRepresentationRecordWire"
)]
pub(crate) struct UfrxRepresentationRecord {
    pub(crate) prefix: u16,
    pub(crate) active_representation: Option<(String, String)>,
    pub(crate) secondary_active_lod_state: [u16; 2],
    pub(crate) active_model_state: String,
    pub(crate) active_model_state_state: [u16; 2],
}

#[derive(Serialize, Deserialize)]
struct UfrxRepresentationRecordWire {
    prefix: u16,
    active_representation: Option<String>,
    active_representation_kind: Option<String>,
    secondary_active_lod_state: [u16; 2],
    active_model_state: String,
    active_model_state_state: [u16; 2],
}

impl From<UfrxRepresentationRecord> for UfrxRepresentationRecordWire {
    fn from(value: UfrxRepresentationRecord) -> Self {
        let (active_representation, active_representation_kind) = value
            .active_representation
            .map_or((None, None), |(name, kind)| (Some(name), Some(kind)));
        Self {
            prefix: value.prefix,
            active_representation,
            active_representation_kind,
            secondary_active_lod_state: value.secondary_active_lod_state,
            active_model_state: value.active_model_state,
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
                (Some(name), Some(kind)) => Some((name, kind)),
                _ => return Err(
                    "active_representation and active_representation_kind must be present together"
                        .into(),
                ),
            };
        Ok(Self {
            prefix: wire.prefix,
            active_representation,
            secondary_active_lod_state: wire.secondary_active_lod_state,
            active_model_state: wire.active_model_state,
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
                tail_sha256: Some(tail_sha256.clone()),
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
                tail_sha256: Some(tail_sha256.clone()),
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
                tail_sha256: wire
                    .tail_sha256
                    .ok_or_else(|| "parsed UFRxDoc requires tail_sha256".to_owned())?,
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
                tail_sha256: wire
                    .tail_sha256
                    .ok_or_else(|| "unsupported UFRxDoc requires tail_sha256".to_owned())?,
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
    fn model_state_admission_rejects_invalid_framing() {
        let valid = serde_json::json!({
            "id": "state", "ordinal": 0, "prefix": 0, "name": "Primary",
            "state": [0, 0], "prefix_count": 0, "parameters": [],
            "suffix_len": 77, "suffix_sha256": "a".repeat(64)
        });
        let record: UfrxModelStateRecord = serde_json::from_value(valid.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), valid);
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
            .unwrap()],
            external_references: vec![],
            embedded_references: vec![],
            occurrences: vec![],
            tail_len: 0,
            tail_sha256: "0".repeat(64),
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
                tail_sha256: "0".repeat(64),
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
