// SPDX-License-Identifier: Apache-2.0
//! `UFRx` document states and their owned child records.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::hash::digest::Sha256Digest;

use cadmpeg_ir::native::{NativeConvertError, NativeNamespace, NativeRecord};
use serde::{ser::SerializeStruct, Deserialize, Serialize};
use std::num::NonZeroU64;

/// Hexadecimal text for exactly sixteen identifier bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Identifier16(NonBlankString);

impl Identifier16 {
    fn try_new(value: String) -> Result<Self, CodecError> {
        if value.len() != 32
            || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(CodecError::Malformed(
                "identifier must contain 32 hexadecimal digits".into(),
            ));
        }
        match NonBlankString::from_ascii_leading(value) {
            Some(value) => Ok(Self(value)),
            None => Err(CodecError::Malformed(
                "identifier must contain 32 hexadecimal digits".into(),
            )),
        }
    }

    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Nonzero document identity for an external reference with no path.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NonzeroDocumentId(Identifier16);

impl NonzeroDocumentId {
    fn try_new(value: Identifier16) -> Option<Self> {
        if value.as_str().bytes().all(|byte| byte == b'0') {
            None
        } else {
            Some(Self(value))
        }
    }

    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

pub(crate) fn embedded_reference_issue(record_len: u64) -> Option<&'static str> {
    (record_len == 0).then_some("record_len must not be zero")
}

pub(crate) fn occurrence_issue(header_padding_words: u8, record_len: u64) -> Option<&'static str> {
    if header_padding_words > 8 {
        Some("header_padding_words must not exceed 8")
    } else {
        embedded_reference_issue(record_len)
    }
}

fn qualify_error(error: CodecError, field: &str) -> CodecError {
    match error {
        CodecError::Malformed(detail) => CodecError::malformed(format_args!("{field}: {detail}")),
        error => error,
    }
}

fn required<T>(value: Option<T>, detail: &str) -> Result<T, CodecError> {
    value.ok_or_else(|| CodecError::malformed(detail))
}

fn native_conversion_error(error: CodecError) -> NativeConvertError {
    match error {
        CodecError::Malformed(detail) => NativeConvertError::ConversionMessage(detail),
        error => NativeConvertError::Resource(error),
    }
}

fn arena_conversion_error(
    ctx: &DecodeContext<'_>,
    arena: &str,
    record: &NativeRecord,
    error: CodecError,
) -> Result<NativeConvertError, NativeConvertError> {
    if matches!(&error, CodecError::ResourceLimit(_)) {
        return Err(NativeConvertError::Resource(error));
    }
    let message = match error {
        CodecError::Malformed(message) => message,
        error => {
            error.to_string()
        }
    };
    let source = NativeConvertError::ReadRecordMessage {
        id: record.identity_for_decode(ctx, "retain Inventor native record error identity")?,
        message,
    };
    let arena = arena.to_owned();
    Ok(NativeConvertError::Arena {
        arena,
        source: Box::new(source),
    })
}

fn convert_arena<Wire, Record>(
    ctx: &DecodeContext<'_>,
    namespace: &NativeNamespace,
    arena: &'static str,
    operation: &'static str,
    mut convert: impl FnMut(Wire, &DecodeContext<'_>) -> Result<Record, CodecError>,
) -> Result<Vec<Record>, NativeConvertError>
where
    Wire: serde::de::DeserializeOwned,
{
    let records = ctx
        .get_btree_map(namespace.arenas(), arena, "find Inventor UFRx record arena")?
        .map_or(&[][..], Vec::as_slice);
    let records = ctx
        .admit_iter(records, operation)
        .map_err(CodecError::from)?;
    let wires = namespace.arena_iter_as_for_decode::<Wire>(ctx, arena);
    ctx.try_collect_vec(
        records.zip(wires).map(|(record, wire)| {
            let wire = wire?;
            match convert(wire, ctx) {
                Ok(record) => Ok(record),
                Err(error) => Err(arena_conversion_error(ctx, arena, record, error)?),
            }
        }),
        operation,
    )
}

fn convert_single_arena<Wire, Record>(
    ctx: &DecodeContext<'_>,
    namespace: &NativeNamespace,
    arena: &'static str,
    operation: &'static str,
    mut convert: impl FnMut(Wire, &DecodeContext<'_>) -> Result<Record, CodecError>,
) -> Result<(Option<Record>, usize), NativeConvertError>
where
    Wire: serde::de::DeserializeOwned,
{
    let records = ctx
        .get_btree_map(namespace.arenas(), arena, "find Inventor UFRx record arena")?
        .map_or(&[][..], Vec::as_slice);
    let count = records.len();
    let records = ctx
        .admit_iter(records, operation)
        .map_err(CodecError::from)?;
    let wires = namespace.arena_iter_as_for_decode::<Wire>(ctx, arena);
    let mut first = None;
    for (record, wire) in records.zip(wires) {
        let wire = wire?;
        let value = match convert(wire, ctx) {
            Ok(value) => value,
            Err(error) => return Err(arena_conversion_error(ctx, arena, record, error)?),
        };
        if first.is_none() {
            first = Some(value);
        }
    }
    Ok((first, count))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UfrxRecord {
    Absent {
        id: String,
    },
    ParsedPrefix(Box<UfrxParsedPrefix>),
    Unsupported {
        id: String,
        directory_id: u32,
        schema: u16,
        section_versions: Vec<u16>,
        tail_len: u64,
        tail_sha256: Sha256Digest,
        detail: String,
    },
    Malformed {
        id: String,
        directory_id: u32,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UfrxParsedPrefix {
    pub(crate) id: String,
    pub(crate) directory_id: u32,
    pub(crate) schema: u16,
    pub(crate) section_versions: Vec<u16>,
    pub(crate) original_file_name: String,
    pub(crate) caption: String,
    pub(crate) representation: Option<UfrxRepresentationRecord>,
    pub(crate) model_states: Vec<UfrxModelStateRecord>,
    pub(crate) external_references: Vec<ExternalReferenceRecord>,
    pub(crate) embedded_references: Vec<EmbeddedReferenceRecord>,
    pub(crate) occurrences: Vec<UfrxOccurrenceRecord>,
    pub(crate) tail_len: u64,
    pub(crate) tail_sha256: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UfrxRepresentationRecord {
    prefix: u16,
    pub(crate) active_representation: Option<(NonBlankString, NonBlankString)>,
    secondary_active_lod_state: [u16; 2],
    active_model_state: NonBlankString,
    active_model_state_state: [u16; 2],
}

impl Serialize for UfrxRepresentationRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (active_representation, active_representation_kind) = self
            .active_representation
            .as_ref()
            .map_or((None, None), |(name, kind)| {
                (Some(name.as_str()), Some(kind.as_str()))
            });
        let mut fields = serializer.serialize_struct("UfrxRepresentationRecordWire", 6)?;
        fields.serialize_field("prefix", &self.prefix)?;
        fields.serialize_field("active_representation", &active_representation)?;
        fields.serialize_field("active_representation_kind", &active_representation_kind)?;
        fields.serialize_field(
            "secondary_active_lod_state",
            &self.secondary_active_lod_state,
        )?;
        fields.serialize_field("active_model_state", self.active_model_state.as_str())?;
        fields.serialize_field("active_model_state_state", &self.active_model_state_state)?;
        fields.end()
    }
}

#[derive(Deserialize)]
pub(crate) struct UfrxRepresentationRecordWire {
    pub(crate) prefix: u16,
    pub(crate) active_representation: Option<String>,
    pub(crate) active_representation_kind: Option<String>,
    pub(crate) secondary_active_lod_state: [u16; 2],
    pub(crate) active_model_state: String,
    pub(crate) active_model_state_state: [u16; 2],
}

impl UfrxRepresentationRecordWire {
    pub(crate) fn into_record(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<UfrxRepresentationRecord, CodecError> {
        let active_representation =
            match (self.active_representation, self.active_representation_kind) {
                (None, None) => None,
                (Some(name), Some(kind)) => {
                    let name = required(NonBlankString::for_decode(ctx, name, "validate active_representation")?, "active_representation must not be empty")?;
                    let kind = required(NonBlankString::for_decode(
                            ctx,
                            kind,
                            "validate active_representation_kind",
                        )?, "active_representation_kind must not be empty")?;
                    Some((name, kind))
                }
                _ => {
                    return Err(CodecError::malformed("active_representation and active_representation_kind must be present together"));
                }
            };
        let active_model_state = required(NonBlankString::for_decode(
                ctx,
                self.active_model_state,
                "validate active_model_state",
            )?, "active_model_state must not be empty")?;
        Ok(UfrxRepresentationRecord {
            prefix: self.prefix,
            active_representation,
            secondary_active_lod_state: self.secondary_active_lod_state,
            active_model_state,
            active_model_state_state: self.active_model_state_state,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UfrxModelStateRecord {
    id: String,
    pub(crate) ordinal: u32,
    prefix: u8,
    name: NonBlankString,
    state: [u16; 2],
    prefix_count: u32,
    parameters: Vec<UfrxModelStateParameterRecord>,
    suffix_sha256: Sha256Digest,
}

impl Serialize for UfrxModelStateRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields = serializer.serialize_struct("UfrxModelStateRecordWire", 9)?;
        fields.serialize_field("id", &self.id)?;
        fields.serialize_field("ordinal", &self.ordinal)?;
        fields.serialize_field("prefix", &self.prefix)?;
        fields.serialize_field("name", self.name.as_str())?;
        fields.serialize_field("state", &self.state)?;
        fields.serialize_field("prefix_count", &self.prefix_count)?;
        fields.serialize_field("parameters", &self.parameters)?;
        fields.serialize_field("suffix_len", &77_u64)?;
        fields.serialize_field("suffix_sha256", &self.suffix_sha256)?;
        fields.end()
    }
}

#[derive(Deserialize)]
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

impl UfrxModelStateRecordWire {
    pub(crate) fn into_record(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<UfrxModelStateRecord, CodecError> {
        if self.suffix_len != 77 {
            return Err(CodecError::malformed("suffix_len must be 77"));
        }
        let name = required(NonBlankString::for_decode(ctx, self.name, "validate name")?, "name must not be empty")?;
        let suffix_sha256 = match Sha256Digest::try_from(self.suffix_sha256) {
            Ok(digest) => digest,
            Err(error) => {
                return Err(CodecError::malformed(format_args!("suffix_sha256: {error}")));
            }
        };
        Ok(UfrxModelStateRecord {
            id: self.id,
            ordinal: self.ordinal,
            prefix: self.prefix,
            name,
            state: self.state,
            prefix_count: self.prefix_count,
            parameters: self.parameters,
            suffix_sha256,
        })
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

#[derive(Deserialize)]
struct UfrxRecordWire {
    id: String,
    state: UfrxRecordState,
    directory_id: Option<u32>,
    schema: Option<u16>,
    section_versions: Vec<u16>,
    original_file_name: Option<String>,
    caption: Option<String>,
    representation: Option<UfrxRepresentationRecordWire>,
    model_state_count: u64,
    reference_count: u64,
    embedded_reference_count: u64,
    occurrence_count: u64,
    tail_len: u64,
    tail_sha256: Option<String>,
    detail: Option<String>,
}

#[derive(Serialize)]
struct UfrxRecordView<'a> {
    id: &'a str,
    state: UfrxRecordState,
    directory_id: Option<u32>,
    schema: Option<u16>,
    section_versions: &'a [u16],
    original_file_name: Option<&'a str>,
    caption: Option<&'a str>,
    representation: Option<&'a UfrxRepresentationRecord>,
    model_state_count: u64,
    reference_count: u64,
    embedded_reference_count: u64,
    occurrence_count: u64,
    tail_len: u64,
    tail_sha256: Option<&'a Sha256Digest>,
    detail: Option<&'a str>,
}

impl<'a> TryFrom<&'a UfrxRecord> for UfrxRecordView<'a> {
    type Error = std::num::TryFromIntError;

    fn try_from(value: &'a UfrxRecord) -> Result<Self, Self::Error> {
        Ok(match value {
            UfrxRecord::Absent { id } => Self {
                id,
                state: UfrxRecordState::Absent,
                directory_id: None,
                schema: None,
                section_versions: &[],
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
            UfrxRecord::ParsedPrefix(payload) => {
                let UfrxParsedPrefix {
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
                } = payload.as_ref();
                Self {
                    id,
                    state: UfrxRecordState::ParsedPrefix,
                    directory_id: Some(*directory_id),
                    schema: Some(*schema),
                    section_versions,
                    original_file_name: Some(original_file_name),
                    caption: Some(caption),
                    representation: representation.as_ref(),
                    model_state_count: cadmpeg_core::decode::u64_from_index(model_states.len()),
                    reference_count: cadmpeg_core::decode::u64_from_index(
                        external_references.len(),
                    ),
                    embedded_reference_count: cadmpeg_core::decode::u64_from_index(
                        embedded_references.len(),
                    ),
                    occurrence_count: cadmpeg_core::decode::u64_from_index(occurrences.len()),
                    tail_len: *tail_len,
                    tail_sha256: Some(tail_sha256),
                    detail: None,
                }
            }
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
                directory_id: Some(*directory_id),
                schema: Some(*schema),
                section_versions,
                original_file_name: None,
                caption: None,
                representation: None,
                model_state_count: 0,
                reference_count: 0,
                embedded_reference_count: 0,
                occurrence_count: 0,
                tail_len: *tail_len,
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
                directory_id: Some(*directory_id),
                schema: None,
                section_versions: &[],
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
        })
    }
}

impl UfrxRecordWire {
    fn into_record(
        self,
        representation: Option<UfrxRepresentationRecord>,
        model_states: Vec<UfrxModelStateRecord>,
        external_references: Vec<ExternalReferenceRecord>,
        embedded_references: Vec<EmbeddedReferenceRecord>,
        occurrences: Vec<UfrxOccurrenceRecord>,
    ) -> Result<UfrxRecord, CodecError> {
        let wire = self;
        if wire.model_state_count != cadmpeg_core::decode::u64_from_index(model_states.len()) {
            return Err(CodecError::malformed("UFRx model_state_count does not match its arena"));
        }
        if wire.reference_count != cadmpeg_core::decode::u64_from_index(external_references.len()) {
            return Err(CodecError::malformed("UFRx reference_count does not match its arena"));
        }
        if wire.embedded_reference_count
            != cadmpeg_core::decode::u64_from_index(embedded_references.len())
        {
            return Err(CodecError::malformed("UFRx embedded_reference_count does not match its arena"));
        }
        if wire.occurrence_count != cadmpeg_core::decode::u64_from_index(occurrences.len()) {
            return Err(CodecError::malformed("UFRx occurrence_count does not match its arena"));
        }
        let has_children = !model_states.is_empty()
            || !external_references.is_empty()
            || !embedded_references.is_empty()
            || !occurrences.is_empty();
        let has_parsed_fields = wire.original_file_name.is_some()
            || wire.caption.is_some()
            || representation.is_some()
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
            return Err(CodecError::malformed("UFRx state carries incompatible fields"));
        }
        match wire.state {
            UfrxRecordState::Absent => Ok(UfrxRecord::Absent { id: wire.id }),
            UfrxRecordState::ParsedPrefix => {
                let directory_id = required(wire.directory_id, "parsed UFRxDoc requires directory_id")?;
                let schema = required(wire.schema, "parsed UFRxDoc requires schema")?;
                let original_file_name = required(wire.original_file_name, "parsed UFRxDoc requires original_file_name")?;
                let caption = required(wire.caption, "parsed UFRxDoc requires caption")?;
                let tail_sha256 = required(wire.tail_sha256, "parsed UFRxDoc requires tail_sha256")?;
                let tail_sha256 = match Sha256Digest::try_from(tail_sha256) {
                    Ok(digest) => digest,
                    Err(error) => {
                        return Err(CodecError::malformed(format_args!("tail_sha256: {error}")));
                    }
                };
                Ok(UfrxRecord::ParsedPrefix(Box::new(UfrxParsedPrefix {
                    id: wire.id,
                    directory_id,
                    schema,
                    section_versions: wire.section_versions,
                    original_file_name,
                    caption,
                    representation,
                    model_states,
                    external_references,
                    embedded_references,
                    occurrences,
                    tail_len: wire.tail_len,
                    tail_sha256,
                })))
            }
            UfrxRecordState::Unsupported => {
                let directory_id = required(wire.directory_id, "unsupported UFRxDoc requires directory_id")?;
                let schema = required(wire.schema, "unsupported UFRxDoc requires schema")?;
                let tail_sha256 = required(wire.tail_sha256, "unsupported UFRxDoc requires tail_sha256")?;
                let tail_sha256 = match Sha256Digest::try_from(tail_sha256) {
                    Ok(digest) => digest,
                    Err(error) => {
                        return Err(CodecError::malformed(format_args!("tail_sha256: {error}")));
                    }
                };
                let detail = required(wire.detail, "unsupported UFRxDoc requires detail")?;
                Ok(UfrxRecord::Unsupported {
                    id: wire.id,
                    directory_id,
                    schema,
                    section_versions: wire.section_versions,
                    tail_len: wire.tail_len,
                    tail_sha256,
                    detail,
                })
            }
            UfrxRecordState::Malformed => {
                let directory_id = required(wire.directory_id, "malformed UFRxDoc requires directory_id")?;
                let detail = required(wire.detail, "malformed UFRxDoc requires detail")?;
                Ok(UfrxRecord::Malformed {
                    id: wire.id,
                    directory_id,
                    detail,
                })
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExternalReferenceRecord {
    id: String,
    ordinal: u32,
    identity: ExternalReferenceIdentity,
    library_id: i32,
    library_name: String,
    display_name: String,
    state_groups: Vec<[u16; 3]>,
    pub(crate) state: [u16; 2],
    database_id: Identifier16,
    pub(crate) reference_id: u32,
    pub(crate) occurrence_count: u32,
    version: u32,
    flags: u32,
}

impl Serialize for ExternalReferenceRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (path, document_id) = match &self.identity {
            ExternalReferenceIdentity::Path { path, document_id } => (
                path.as_str(),
                document_id.as_ref().map(NonzeroDocumentId::as_str),
            ),
            ExternalReferenceIdentity::DocumentId(document_id) => ("", Some(document_id.as_str())),
        };
        let mut fields = serializer.serialize_struct(
            "ExternalReferenceRecordWire",
            if document_id.is_some() { 14 } else { 13 },
        )?;
        fields.serialize_field("id", &self.id)?;
        fields.serialize_field("ordinal", &self.ordinal)?;
        fields.serialize_field("path", path)?;
        fields.serialize_field("library_id", &self.library_id)?;
        fields.serialize_field("library_name", &self.library_name)?;
        fields.serialize_field("display_name", &self.display_name)?;
        fields.serialize_field("state_groups", &self.state_groups)?;
        fields.serialize_field("state", &self.state)?;
        if let Some(document_id) = document_id {
            fields.serialize_field("document_id", document_id)?;
        }
        fields.serialize_field("database_id", self.database_id.as_str())?;
        fields.serialize_field("reference_id", &self.reference_id)?;
        fields.serialize_field("occurrence_count", &self.occurrence_count)?;
        fields.serialize_field("version", &self.version)?;
        fields.serialize_field("flags", &self.flags)?;
        fields.end()
    }
}

#[derive(Deserialize)]
pub(crate) struct ExternalReferenceRecordWire {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    pub(crate) path: String,
    pub(crate) library_id: i32,
    pub(crate) library_name: String,
    pub(crate) display_name: String,
    pub(crate) state_groups: Vec<[u16; 3]>,
    pub(crate) state: [u16; 2],
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_document_id"
    )]
    pub(crate) document_id: Option<String>,
    pub(crate) database_id: String,
    pub(crate) reference_id: u32,
    pub(crate) occurrence_count: u32,
    pub(crate) version: u32,
    pub(crate) flags: u32,
}

cadmpeg_core::named_optional_field!(deserialize_document_id, String, "document_id");

impl ExternalReferenceRecordWire {
    pub(crate) fn into_record(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<ExternalReferenceRecord, CodecError> {
        let suffix = required(ctx.strip_prefix(
                &self.id,
                "inventor:ufrx:external-reference#",
                "validate Inventor UFRx external reference identity prefix",
            )?, "external reference id has an invalid namespace")?;
        if suffix.is_empty() {
            return Err(CodecError::malformed("external reference id disagrees with ordinal"));
        }
        let digits = ctx.all_by(
            suffix.as_bytes(),
            |byte| Ok(byte.is_ascii_digit()),
            "validate Inventor UFRx external reference ordinal digits",
        )?;
        if !digits {
            return Err(CodecError::malformed("external reference id disagrees with ordinal"));
        }
        if suffix.len() > 1
            && ctx.starts_with(
                suffix,
                "0",
                "validate Inventor UFRx external reference ordinal spelling",
            )?
        {
            return Err(CodecError::malformed("external reference id disagrees with ordinal"));
        }
        let parsed_ordinal =
            ctx.parse_text::<u32>(suffix, "parse Inventor UFRx external reference ordinal")?;
        if parsed_ordinal.ok() != Some(self.ordinal) {
            return Err(CodecError::malformed("external reference id disagrees with ordinal"));
        }

        let database_id = match Identifier16::try_new(self.database_id) {
            Ok(value) => value,
            Err(error) => {
                return Err(qualify_error(error, "database_id"));
            }
        };
        let document_id = match self.document_id {
            Some(value) => {
                let value = match Identifier16::try_new(value) {
                    Ok(value) => value,
                    Err(error) => {
                        return Err(qualify_error(error, "document_id"));
                    }
                };
                NonzeroDocumentId::try_new(value)
            }
            None => None,
        };
        let path = NonBlankString::for_decode(ctx, self.path, "validate path")?;
        let identity = match (path, document_id) {
            (Some(path), document_id) => ExternalReferenceIdentity::Path { path, document_id },
            (None, Some(document_id)) => ExternalReferenceIdentity::DocumentId(document_id),
            (None, None) => {
                return Err(CodecError::malformed("path or a nonzero document_id is required"));
            }
        };
        Ok(ExternalReferenceRecord {
            id: self.id,
            ordinal: self.ordinal,
            identity,
            library_id: self.library_id,
            library_name: self.library_name,
            display_name: self.display_name,
            state_groups: self.state_groups,
            state: self.state,
            database_id,
            reference_id: self.reference_id,
            occurrence_count: self.occurrence_count,
            version: self.version,
            flags: self.flags,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ExternalReferenceIdentity {
    Path {
        path: NonBlankString,
        document_id: Option<NonzeroDocumentId>,
    },
    DocumentId(NonzeroDocumentId),
}

impl ExternalReferenceRecord {
    pub(crate) fn id(&self) -> &String {
        &self.id
    }

    pub(crate) fn ordinal(&self) -> u32 {
        self.ordinal
    }

    pub(crate) fn document(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<cadmpeg_ir::products::ExternalDocument, CodecError> {
        use cadmpeg_ir::products::ExternalDocument;
        Ok(match &self.identity {
            ExternalReferenceIdentity::Path { path, .. } => ExternalDocument::Path {
                path: path
                    .try_clone_for_decode(ctx, "copy Inventor UFRx external document path")?,
            },
            ExternalReferenceIdentity::DocumentId(document_id) => ExternalDocument::DocumentId {
                document_id: document_id
                    .0
                     .0
                    .try_clone_for_decode(ctx, "copy Inventor UFRx external document identifier")?,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EmbeddedReferenceRecord {
    id: String,
    pub(crate) ordinal: u32,
    value_0: u32,
    filetime: u64,
    value_1: u32,
    extended_value: Option<u32>,
    value_2: u32,
    path: String,
    library_id: i32,
    library_name: String,
    state: u16,
    display_name: String,
    state_values: [u8; 8],
    record_len: NonZeroU64,
    record_sha256: Sha256Digest,
}

impl Serialize for EmbeddedReferenceRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields = serializer.serialize_struct("EmbeddedReferenceRecordWire", 15)?;
        fields.serialize_field("id", &self.id)?;
        fields.serialize_field("ordinal", &self.ordinal)?;
        fields.serialize_field("value_0", &self.value_0)?;
        fields.serialize_field("filetime", &self.filetime)?;
        fields.serialize_field("value_1", &self.value_1)?;
        fields.serialize_field("extended_value", &self.extended_value)?;
        fields.serialize_field("value_2", &self.value_2)?;
        fields.serialize_field("path", &self.path)?;
        fields.serialize_field("library_id", &self.library_id)?;
        fields.serialize_field("library_name", &self.library_name)?;
        fields.serialize_field("state", &self.state)?;
        fields.serialize_field("display_name", &self.display_name)?;
        fields.serialize_field("state_values", &self.state_values)?;
        fields.serialize_field("record_len", &self.record_len.get())?;
        fields.serialize_field("record_sha256", &self.record_sha256)?;
        fields.end()
    }
}

#[derive(Deserialize)]
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

impl EmbeddedReferenceRecordWire {
    pub(crate) fn into_record(
        self,
        _ctx: &DecodeContext<'_>,
    ) -> Result<EmbeddedReferenceRecord, CodecError> {
        if let Some(issue) = embedded_reference_issue(self.record_len) {
            return Err(CodecError::malformed(issue));
        }
        let Some(record_len) = NonZeroU64::new(self.record_len) else {
            return Err(CodecError::malformed("record_len must not be zero"));
        };
        let record_sha256 = match Sha256Digest::try_from(self.record_sha256) {
            Ok(digest) => digest,
            Err(error) => {
                return Err(CodecError::malformed(format_args!("record_sha256: {error}")));
            }
        };
        Ok(EmbeddedReferenceRecord {
            id: self.id,
            ordinal: self.ordinal,
            value_0: self.value_0,
            filetime: self.filetime,
            value_1: self.value_1,
            extended_value: self.extended_value,
            value_2: self.value_2,
            path: self.path,
            library_id: self.library_id,
            library_name: self.library_name,
            state: self.state,
            display_name: self.display_name,
            state_values: self.state_values,
            record_len,
            record_sha256,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UfrxOccurrenceRecord {
    pub(crate) id: String,
    pub(crate) ordinal: u32,
    end_string_flag: u32,
    pub(crate) file_reference_id: u32,
    pub(crate) occurrence_id: u32,
    header_value: u32,
    pub(crate) title: Option<String>,
    header_padding_words: u8,
    record_len: NonZeroU64,
    record_sha256: Sha256Digest,
}

impl Serialize for UfrxOccurrenceRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields = serializer.serialize_struct("UfrxOccurrenceRecordWire", 10)?;
        fields.serialize_field("id", &self.id)?;
        fields.serialize_field("ordinal", &self.ordinal)?;
        fields.serialize_field("end_string_flag", &self.end_string_flag)?;
        fields.serialize_field("file_reference_id", &self.file_reference_id)?;
        fields.serialize_field("occurrence_id", &self.occurrence_id)?;
        fields.serialize_field("header_value", &self.header_value)?;
        fields.serialize_field("title", &self.title)?;
        fields.serialize_field("header_padding_words", &self.header_padding_words)?;
        fields.serialize_field("record_len", &self.record_len.get())?;
        fields.serialize_field("record_sha256", &self.record_sha256)?;
        fields.end()
    }
}

#[derive(Deserialize)]
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

impl UfrxOccurrenceRecordWire {
    pub(crate) fn into_record(
        self,
        _ctx: &DecodeContext<'_>,
    ) -> Result<UfrxOccurrenceRecord, CodecError> {
        if let Some(issue) = occurrence_issue(self.header_padding_words, self.record_len) {
            return Err(CodecError::malformed(issue));
        }
        let Some(record_len) = NonZeroU64::new(self.record_len) else {
            return Err(CodecError::malformed("record_len must not be zero"));
        };
        let record_sha256 = match Sha256Digest::try_from(self.record_sha256) {
            Ok(digest) => digest,
            Err(error) => {
                return Err(CodecError::malformed(format_args!("record_sha256: {error}")));
            }
        };
        Ok(UfrxOccurrenceRecord {
            id: self.id,
            ordinal: self.ordinal,
            end_string_flag: self.end_string_flag,
            file_reference_id: self.file_reference_id,
            occurrence_id: self.occurrence_id,
            header_value: self.header_value,
            title: self.title,
            header_padding_words: self.header_padding_words,
            record_len,
            record_sha256,
        })
    }
}

impl Serialize for UfrxRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        UfrxRecordView::try_from(self)
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}
impl UfrxRecord {
    pub(crate) fn model_states(&self) -> &[UfrxModelStateRecord] {
        match self {
            Self::ParsedPrefix(payload) => &payload.model_states,
            _ => &[],
        }
    }
    pub(crate) fn external_references(&self) -> &[ExternalReferenceRecord] {
        match self {
            Self::ParsedPrefix(payload) => &payload.external_references,
            _ => &[],
        }
    }
    pub(crate) fn embedded_references(&self) -> &[EmbeddedReferenceRecord] {
        match self {
            Self::ParsedPrefix(payload) => &payload.embedded_references,
            _ => &[],
        }
    }
    pub(crate) fn occurrences(&self) -> &[UfrxOccurrenceRecord] {
        match self {
            Self::ParsedPrefix(payload) => &payload.occurrences,
            _ => &[],
        }
    }
    pub(crate) fn install(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        namespace: &mut NativeNamespace,
    ) -> Result<(), NativeConvertError> {
        namespace.set_arena(ctx, "ufrx", std::slice::from_ref(self))?;
        namespace.set_arena(ctx, "ufrx_model_states", self.model_states())?;
        namespace.set_arena(ctx, "external_references", self.external_references())?;
        namespace.set_arena(ctx, "embedded_references", self.embedded_references())?;
        namespace.set_arena(ctx, "ufrx_occurrences", self.occurrences())?;
        Ok(())
    }
    pub(crate) fn read(
        ctx: &DecodeContext<'_>,
        namespace: &NativeNamespace,
    ) -> Result<Self, NativeConvertError> {
        let (record, record_count) = convert_single_arena::<UfrxRecordWire, _>(
            ctx,
            namespace,
            "ufrx",
            "convert Inventor UFRx state records",
            |mut wire, ctx| {
                let representation = wire
                    .representation
                    .take()
                    .map(|representation| representation.into_record(ctx))
                    .transpose()?;
                Ok((wire, representation))
            },
        )?;
        if record_count != 1 {
            return Err(NativeConvertError::ConversionMessage(std::fmt::format(format_args!("Inventor native data has {record_count} UFRxDoc state records"))));
        }
        let Some((wire, representation)) = record else {
            return Err(NativeConvertError::ConversionMessage("native record disappeared during UFRx conversion".into()));
        };
        let model_states = convert_arena::<UfrxModelStateRecordWire, _>(
            ctx,
            namespace,
            "ufrx_model_states",
            "convert Inventor UFRx model states",
            UfrxModelStateRecordWire::into_record,
        )?;
        let external_references = convert_arena::<ExternalReferenceRecordWire, _>(
            ctx,
            namespace,
            "external_references",
            "convert Inventor UFRx external references",
            ExternalReferenceRecordWire::into_record,
        )?;
        let embedded_references = convert_arena::<EmbeddedReferenceRecordWire, _>(
            ctx,
            namespace,
            "embedded_references",
            "convert Inventor UFRx embedded references",
            EmbeddedReferenceRecordWire::into_record,
        )?;
        let occurrences = convert_arena::<UfrxOccurrenceRecordWire, _>(
            ctx,
            namespace,
            "ufrx_occurrences",
            "convert Inventor UFRx occurrences",
            UfrxOccurrenceRecordWire::into_record,
        )?;
        wire.into_record(
            representation,
            model_states,
            external_references,
            embedded_references,
            occurrences,
        )
        .map_err(native_conversion_error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        embedded_reference_issue, occurrence_issue, EmbeddedReferenceRecord,
        EmbeddedReferenceRecordWire, ExternalReferenceRecord, ExternalReferenceRecordWire,
        UfrxModelStateRecord, UfrxModelStateRecordWire, UfrxOccurrenceRecord,
        UfrxOccurrenceRecordWire, UfrxParsedPrefix, UfrxRecord, UfrxRepresentationRecord,
        UfrxRepresentationRecordWire,
    };
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::hash::digest::Sha256Digest;
    use cadmpeg_ir::native::NativeNamespace;
    use cadmpeg_test_support::native_serialization::assert_native_limit;
    use cadmpeg_test_support::refusal::{refusal, states_the_key};
    use serde::de::DeserializeOwned;

    fn from_wire<W: DeserializeOwned, T>(
        value: serde_json::Value,
        convert: impl FnOnce(W, &DecodeContext<'static>) -> Result<T, CodecError>,
    ) -> Result<T, String> {
        let ctx = crate::native::test_ctx();
        let wire = serde_json::from_value::<W>(value).map_err(|error| error.to_string())?;
        convert(wire, &ctx).map_err(|error| error.to_string())
    }

    fn decode_representation(value: serde_json::Value) -> Result<UfrxRepresentationRecord, String> {
        from_wire::<UfrxRepresentationRecordWire, _>(value, |wire, ctx| wire.into_record(ctx))
    }

    fn decode_model_state(value: serde_json::Value) -> Result<UfrxModelStateRecord, String> {
        from_wire::<UfrxModelStateRecordWire, _>(value, |wire, ctx| wire.into_record(ctx))
    }

    fn decode_external_reference(
        value: serde_json::Value,
    ) -> Result<ExternalReferenceRecord, String> {
        from_wire::<ExternalReferenceRecordWire, _>(value, |wire, ctx| wire.into_record(ctx))
    }

    fn decode_embedded_reference(
        value: serde_json::Value,
    ) -> Result<EmbeddedReferenceRecord, String> {
        from_wire::<EmbeddedReferenceRecordWire, _>(value, |wire, ctx| wire.into_record(ctx))
    }

    fn decode_occurrence(value: serde_json::Value) -> Result<UfrxOccurrenceRecord, String> {
        from_wire::<UfrxOccurrenceRecordWire, _>(value, |wire, ctx| wire.into_record(ctx))
    }

    #[test]
    fn ufrx_cardinality_error_uses_no_retained_bytes() {
        use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert!(matches!(UfrxRecord::read(&ctx, &NativeNamespace::default()),
            Err(cadmpeg_ir::native::NativeConvertError::ConversionMessage(detail))
                if detail == "Inventor native data has 0 UFRxDoc state records"));
    }

    #[test]
    fn ufrx_identifiers_validate_fixed_width_without_admission() {
        let zero = super::Identifier16::try_new("0".repeat(32)).expect("identifier");
        assert!(super::NonzeroDocumentId::try_new(zero).is_none());
        let nonzero = super::Identifier16::try_new("0123456789ABCDEF0123456789abcdef".into()).expect("identifier");
        assert!(super::NonzeroDocumentId::try_new(nonzero).is_some());
        for value in ["0".repeat(31), "0".repeat(33), "g".repeat(32)] {
            assert!(super::Identifier16::try_new(value).is_err());
        }
    }

    #[test]
    fn ufrx_conversion_error_keeps_only_variable_identity_admission() {
        use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
        let id = "inventor:ufrx:state#root";
        let mut namespace = NativeNamespace::default();
        namespace.set_arena(&crate::native::test_ctx(), "ufrx", &[serde_json::json!({"id": id})]).expect("record");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The validated identity is copied once. The fixed arena name, box
        // and returned diagnostic require no retained model-byte admission.
        policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(id.len());
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let error = super::arena_conversion_error(&ctx, "ufrx", &namespace.arenas()["ufrx"][0],
            CodecError::malformed("fixed conversion error")).expect("identity fits");
        let message = error.to_string();
        assert!(message.contains(id));
        assert!(message.contains("fixed conversion error"));
    }

    #[test]
    fn ufrx_representation_streams_once_with_retained_limit() {
        #[derive(serde::Serialize)]
        struct Row<'a> {
            id: &'static str,
            representation: &'a UfrxRepresentationRecord,
        }

        let representation = serde_json::json!({
            "prefix": 0, "active_representation": "Master",
            "active_representation_kind": "LOD", "secondary_active_lod_state": [0, 0],
            "active_model_state": "Primary", "active_model_state_state": [0, 0]
        });
        let admitted: UfrxRepresentationRecord =
            from_wire::<UfrxRepresentationRecordWire, _>(representation.clone(), |wire, ctx| {
                wire.into_record(ctx)
            })
            .expect("valid fixture");
        let record = Row {
            id: "inventor:ufrx:representation#0",
            representation: &admitted,
        };
        assert_native_limit(
            &record,
            serde_json::json!({"id": record.id, "representation": representation}),
        );
    }

    #[test]
    fn ufrx_model_state_streams_once_with_retained_limit() {
        let expected = serde_json::json!({
            "id": "inventor:ufrx:model-state#0", "ordinal": 0, "prefix": 0,
            "name": "Primary", "state": [0, 0], "prefix_count": 0,
            "parameters": [], "suffix_len": 77, "suffix_sha256": "a".repeat(64)
        });
        let record: UfrxModelStateRecord =
            from_wire::<UfrxModelStateRecordWire, _>(expected.clone(), |wire, ctx| {
                wire.into_record(ctx)
            })
            .expect("valid fixture");
        assert_native_limit(&record, expected);
    }

    #[test]
    fn ufrx_external_reference_streams_once_with_retained_limit() {
        let expected = serde_json::json!({
            "id": "inventor:ufrx:external-reference#0", "ordinal": 0,
            "path": "part.ipt", "library_id": 0, "library_name": "",
            "display_name": "", "state_groups": [], "state": [0, 0],
            "database_id": "0".repeat(32), "reference_id": 1, "occurrence_count": 0,
            "version": 0, "flags": 0
        });
        let record: ExternalReferenceRecord =
            from_wire::<ExternalReferenceRecordWire, _>(expected.clone(), |wire, ctx| {
                wire.into_record(ctx)
            })
            .expect("valid fixture");
        assert_native_limit(&record, expected);
    }

    #[test]
    fn ufrx_embedded_reference_streams_once_with_retained_limit() {
        let expected = serde_json::json!({
            "id": "inventor:ufrx:embedded-reference#0", "ordinal": 0,
            "value_0": 0, "filetime": 0, "value_1": 0, "extended_value": null,
            "value_2": 0, "path": "", "library_id": 0,
            "library_name": "", "state": 0, "display_name": "",
            "state_values": [0,0,0,0,0,0,0,0], "record_len": 1,
            "record_sha256": "a".repeat(64)
        });
        let record: EmbeddedReferenceRecord =
            from_wire::<EmbeddedReferenceRecordWire, _>(expected.clone(), |wire, ctx| {
                wire.into_record(ctx)
            })
            .expect("valid fixture");
        assert_native_limit(&record, expected);
    }

    #[test]
    fn ufrx_occurrence_streams_once_with_retained_limit() {
        let expected = serde_json::json!({
            "id": "inventor:ufrx:occurrence#0", "ordinal": 0,
            "end_string_flag": 0, "file_reference_id": 1,
            "occurrence_id": 1, "header_value": 0, "title": null,
            "header_padding_words": 8, "record_len": 1,
            "record_sha256": "a".repeat(64)
        });
        let record: UfrxOccurrenceRecord =
            from_wire::<UfrxOccurrenceRecordWire, _>(expected.clone(), |wire, ctx| {
                wire.into_record(ctx)
            })
            .expect("valid fixture");
        assert_native_limit(&record, expected);
    }

    #[test]
    fn ufrx_state_streams_once_with_retained_limit() {
        let record = UfrxRecord::ParsedPrefix(Box::new(UfrxParsedPrefix {
            id: "inventor:ufrx:state#root".into(),
            directory_id: 3,
            schema: 1,
            section_versions: vec![1],
            original_file_name: "part.ipt".into(),
            caption: "part".into(),
            representation: None,
            model_states: vec![],
            external_references: vec![],
            embedded_references: vec![],
            occurrences: vec![],
            tail_len: 0,
            tail_sha256: Sha256Digest::try_from("0".repeat(64)).expect("valid fixture"),
        }));
        let expected = serde_json::to_value(&record).expect("valid fixture");
        assert_native_limit(&record, expected);
    }

    #[test]
    fn ufrx_conversion_issues_preserve_wire_refusal_order() {
        let model = serde_json::json!({
            "id": "state", "ordinal": 0, "prefix": 0, "name": " ",
            "state": [0, 0], "prefix_count": 0, "parameters": [],
            "suffix_len": 76, "suffix_sha256": "a".repeat(64)
        });
        assert!(
            from_wire::<UfrxModelStateRecordWire, _>(model.clone(), |wire, ctx| {
                wire.into_record(ctx)
            })
            .expect_err("wrong suffix length")
            .contains("suffix_len must be 77")
        );
        let mut model = model;
        model["suffix_len"] = serde_json::json!(77);
        assert!(
            from_wire::<UfrxModelStateRecordWire, _>(model, |wire, ctx| { wire.into_record(ctx) })
                .expect_err("blank model name")
                .contains("name must not be empty")
        );
        assert_eq!(
            embedded_reference_issue(0),
            Some("record_len must not be zero")
        );
        assert_eq!(
            occurrence_issue(9, 0),
            Some("header_padding_words must not exceed 8")
        );
        assert_eq!(occurrence_issue(8, 0), Some("record_len must not be zero"));
        let representation = serde_json::json!({
            "prefix": 0, "active_representation": " ",
            "active_representation_kind": " ", "secondary_active_lod_state": [0, 0],
            "active_model_state": " ", "active_model_state_state": [0, 0]
        });
        assert!(
            from_wire::<UfrxRepresentationRecordWire, _>(representation, |wire, ctx| wire
                .into_record(ctx))
            .expect_err("blank representation name")
            .contains("active_representation must not be empty")
        );
        let mut representation = serde_json::json!({
            "prefix": 0, "active_representation": "name",
            "active_representation_kind": null, "secondary_active_lod_state": [0, 0],
            "active_model_state": " ", "active_model_state_state": [0, 0]
        });
        representation["active_representation_kind"] = serde_json::json!(" ");
        assert!(
            from_wire::<UfrxRepresentationRecordWire, _>(representation, |wire, ctx| wire
                .into_record(ctx))
            .expect_err("blank representation kind")
            .contains("active_representation_kind must not be empty")
        );
        let representation = serde_json::json!({
            "prefix": 0, "active_representation": "name",
            "active_representation_kind": null, "secondary_active_lod_state": [0, 0],
            "active_model_state": " ", "active_model_state_state": [0, 0]
        });
        assert!(
            from_wire::<UfrxRepresentationRecordWire, _>(representation, |wire, ctx| wire
                .into_record(ctx))
            .expect_err("half representation pair")
            .contains(
                "active_representation and active_representation_kind must be present together"
            )
        );
        let representation = serde_json::json!({
            "prefix": 0, "active_representation": null,
            "active_representation_kind": null, "secondary_active_lod_state": [0, 0],
            "active_model_state": " ", "active_model_state_state": [0, 0]
        });
        assert!(
            from_wire::<UfrxRepresentationRecordWire, _>(representation, |wire, ctx| wire
                .into_record(ctx))
            .expect_err("blank active model state")
            .contains("active_model_state must not be empty")
        );
        let external = serde_json::json!({
            "id": "inventor:ufrx:external-reference#0", "ordinal": 0,
            "path": " ", "library_id": 0, "library_name": "", "display_name": "",
            "state_groups": [], "state": [0, 0], "document_id": "0".repeat(32),
            "database_id": "0".repeat(32), "reference_id": 1, "occurrence_count": 0,
            "version": 0, "flags": 0
        });
        assert!(
            from_wire::<ExternalReferenceRecordWire, _>(external, |wire, ctx| {
                wire.into_record(ctx)
            })
            .expect_err("blank path and zero document ID")
            .contains("path or a nonzero document_id is required")
        );
    }

    #[test]
    fn external_reference_database_identity_and_ordinal_are_checked() {
        let valid = serde_json::json!({
            "id": "inventor:ufrx:external-reference#0", "ordinal": 0,
            "path": "part.ipt", "library_id": 0, "library_name": "", "display_name": "",
            "state_groups": [], "state": [0, 0], "database_id": "0".repeat(32),
            "reference_id": 1, "occurrence_count": 0, "version": 0, "flags": 0
        });
        let record = decode_external_reference(valid.clone()).expect("zero database ID is valid");
        assert_eq!(serde_json::to_value(record).expect("record"), valid);
        for database_id in ["", "not-hex", "0001", "g0000000000000000000000000000000"] {
            let mut wire = valid.clone();
            wire["database_id"] = serde_json::json!(database_id);
            assert!(decode_external_reference(wire).is_err());
        }
        for id in [
            "",
            "inventor:ufrx:external-reference#7",
            "inventor:ufrx:external-reference#00",
            "inventor:ufrx:external-reference#+0",
        ] {
            let mut wire = valid.clone();
            wire["id"] = serde_json::json!(id);
            assert!(decode_external_reference(wire).is_err());
        }
        let mut wrong_ordinal = valid;
        wrong_ordinal["ordinal"] = serde_json::json!(7);
        assert!(decode_external_reference(wrong_ordinal).is_err());
    }

    #[test]
    fn external_reference_requires_path_or_nonzero_document_id() {
        let valid = serde_json::json!({
            "id": "inventor:ufrx:external-reference#0", "ordinal": 0, "path": "part.ipt", "library_id": 0,
            "library_name": "", "display_name": "", "state_groups": [], "state": [0,0],
            "document_id": "0".repeat(32), "database_id": "0".repeat(32), "reference_id": 1,
            "occurrence_count": 0, "version": 0, "flags": 0
        });
        for (path, document_id, accepted) in [
            ("part.ipt", "0000", false),
            ("part.ipt", "", false),
            ("", "0001", false),
            ("part.ipt", "0001", false),
            ("", "0000", false),
            ("", "", false),
            ("", "garbage", false),
            ("part.ipt", "00000000000000000000000000000000", true),
            ("", "00000000000000000000000000000001", true),
            ("part.ipt", "00000000000000000000000000000001", true),
            ("", "00000000000000000000000000000000", false),
        ] {
            let mut wire = valid.clone();
            wire["path"] = serde_json::json!(path);
            wire["document_id"] = serde_json::json!(document_id);
            let record = decode_external_reference(wire.clone());
            if accepted {
                if document_id.chars().all(|character| character == '0') {
                    wire.as_object_mut()
                        .expect("external-reference object fixture")
                        .remove("document_id");
                }
                assert_eq!(
                    serde_json::to_value(record.expect("valid native record fixture"))
                        .expect("valid native record fixture"),
                    wire
                );
            } else {
                assert!(record
                    .expect_err("invalid native record fixture")
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
            "record_sha256": "a".repeat(64)
        });
        let embedded = serde_json::json!({
            "id": "embedded", "ordinal": 0, "value_0": 0, "filetime": 0,
            "value_1": 0, "extended_value": null, "value_2": 0, "path": "",
            "library_id": 0, "library_name": "", "state": 0, "display_name": "",
            "state_values": [0,0,0,0,0,0,0,0], "record_len": 1,
            "record_sha256": "a".repeat(64)
        });
        let admitted = decode_occurrence(occurrence.clone()).expect("valid native record fixture");
        assert_eq!(
            serde_json::to_value(admitted).expect("valid native record fixture"),
            occurrence
        );
        let admitted =
            decode_embedded_reference(embedded.clone()).expect("valid native record fixture");
        assert_eq!(
            serde_json::to_value(admitted).expect("valid native record fixture"),
            embedded
        );
        for (field, value) in [
            ("record_len", serde_json::json!(0)),
            ("record_sha256", serde_json::json!("a".repeat(63))),
            ("record_sha256", serde_json::json!("g".repeat(64))),
            ("record_sha256", serde_json::json!("A".repeat(64))),
        ] {
            let mut wire = occurrence.clone();
            wire[field] = value.clone();
            assert!(decode_occurrence(wire)
                .expect_err("invalid native record fixture")
                .contains(field));
            let mut wire = embedded.clone();
            wire[field] = value;
            assert!(decode_embedded_reference(wire)
                .expect_err("invalid native record fixture")
                .contains(field));
        }
        let mut wire = occurrence;
        wire["header_padding_words"] = serde_json::json!(9);
        assert!(decode_occurrence(wire)
            .expect_err("invalid native record fixture")
            .contains("header_padding_words"));
    }

    #[test]
    fn representation_admission_rejects_empty_names_and_half_pairs() {
        let valid = serde_json::json!({
            "prefix": 0, "active_representation": "Master",
            "active_representation_kind": "LOD", "secondary_active_lod_state": [0, 0],
            "active_model_state": "Primary", "active_model_state_state": [0, 0]
        });
        let record = decode_representation(valid.clone()).expect("valid native record fixture");
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
            assert!(decode_representation(wire)
                .expect_err("invalid native record fixture")
                .contains(field));
        }
        for field in ["active_representation", "active_representation_kind"] {
            let mut wire = valid.clone();
            wire[field] = serde_json::Value::Null;
            assert!(decode_representation(wire).is_err());
        }
        let mut wire = valid;
        wire["active_representation"] = serde_json::Value::Null;
        wire["active_representation_kind"] = serde_json::Value::Null;
        assert!(decode_representation(wire).is_ok());
    }

    #[test]
    fn model_state_admission_rejects_invalid_framing() {
        let valid = serde_json::json!({
            "id": "state", "ordinal": 0, "prefix": 0, "name": "Primary",
            "state": [0, 0], "prefix_count": 0, "parameters": [],
            "suffix_len": 77, "suffix_sha256": "a".repeat(64)
        });
        let record = decode_model_state(valid.clone()).expect("valid native record fixture");
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
            assert!(decode_model_state(wire).is_err(), "{field}");
        }
    }

    #[test]
    fn parsed_state_owns_arenas_and_checks_wire_counts() {
        let record = UfrxRecord::ParsedPrefix(Box::new(UfrxParsedPrefix {
            id: "inventor:ufrx:state#root".into(),
            directory_id: 3,
            schema: 1,
            section_versions: vec![1],
            original_file_name: "part.ipt".into(),
            caption: "part".into(),
            representation: None,
            model_states: vec![UfrxModelStateRecordWire {
                id: "inventor:ufrx:model-state#0".into(),
                ordinal: 0,
                prefix: 0,
                name: "Primary".into(),
                state: [0, 0],
                prefix_count: 0,
                parameters: vec![],
                suffix_len: 77,
                suffix_sha256: "0".repeat(64),
            }
            .into_record(&crate::native::test_ctx())
            .expect("valid native record fixture")],
            external_references: vec![],
            embedded_references: vec![],
            occurrences: vec![],
            tail_len: 0,
            tail_sha256: Sha256Digest::try_from("0".repeat(64)).expect("64 hexadecimal digits"),
        }));
        let mut namespace = NativeNamespace::default();
        record
            .install(&crate::native::test_ctx(), &mut namespace)
            .expect("valid test fixture");
        assert_eq!(
            UfrxRecord::read(&crate::native::test_ctx(), &namespace).expect("valid test fixture"),
            record
        );
        let mut wire = namespace
            .arena_as::<serde_json::Value>("ufrx")
            .expect("valid test fixture");
        assert_eq!(wire[0]["model_state_count"], 1);
        wire[0]["model_state_count"] = serde_json::json!(0);
        namespace
            .set_arena(&crate::native::test_ctx(), "ufrx", &wire)
            .expect("valid test fixture");
        assert!(UfrxRecord::read(&crate::native::test_ctx(), &namespace)
            .expect_err("invalid test fixture")
            .to_string()
            .contains("model_state_count"));
        let absent = UfrxRecord::Absent {
            id: "inventor:ufrx:state#root".into(),
        };
        namespace
            .set_arena(&crate::native::test_ctx(), "ufrx", &[absent])
            .expect("valid test fixture");
        assert!(UfrxRecord::read(&crate::native::test_ctx(), &namespace).is_err());
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
                tail_sha256: Sha256Digest::try_from("0".repeat(64)).expect("64 hexadecimal digits"),
                detail: "schema".into(),
            },
        ] {
            let mut namespace = NativeNamespace::default();
            record
                .install(&crate::native::test_ctx(), &mut namespace)
                .expect("valid test fixture");
            assert_eq!(
                UfrxRecord::read(&crate::native::test_ctx(), &namespace)
                    .expect("valid test fixture"),
                record
            );
            let mut wire = namespace
                .arena_as::<serde_json::Value>("ufrx")
                .expect("valid test fixture");
            wire[0]["caption"] = serde_json::json!("orphan");
            namespace
                .set_arena(&crate::native::test_ctx(), "ufrx", &wire)
                .expect("valid test fixture");
            assert!(UfrxRecord::read(&crate::native::test_ctx(), &namespace).is_err());
        }
    }

    /// A top-level optional key on a UFRX record names itself in its refusal.
    #[test]
    fn a_top_level_ufrx_key_names_itself_in_its_refusal() {
        states_the_key(
            "document_id",
            &refusal::<super::ExternalReferenceRecordWire>("document_id"),
        );
    }
}

// Each optional key below names itself in whatever it refuses.
