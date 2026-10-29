// SPDX-License-Identifier: Apache-2.0
//! Bounded `UFRxDoc` document and external-reference inventory.

use cadmpeg_container::compound::{CompoundSnapshot, CompoundStreamId};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::rse::DocumentKind;

#[derive(Debug)]
pub(crate) enum UfrxState<'a> {
    Absent,
    Parsed(Box<UfrxDocument<'a>>),
    Unsupported {
        stream: CompoundStreamId,
        schema: u16,
        section_versions: Vec<u16>,
        source: View<'a>,
        detail: String,
    },
    Malformed {
        stream: CompoundStreamId,
        detail: String,
    },
}

#[derive(Debug)]
pub(crate) struct UfrxDocument<'a> {
    pub(crate) stream: CompoundStreamId,
    pub(crate) schema: u16,
    pub(crate) section_versions: Vec<u16>,
    pub(crate) original_file_name: String,
    pub(crate) caption: String,
    pub(crate) representation: Option<UfrxRepresentationState>,
    pub(crate) model_states: Vec<UfrxModelState<'a>>,
    pub(crate) levels_of_detail: Vec<UfrxLevelOfDetail>,
    pub(crate) references: Vec<InventorExternalReference>,
    pub(crate) embedded_references: Vec<InventorEmbeddedReference<'a>>,
    pub(crate) occurrences: Vec<UfrxOccurrence<'a>>,
    pub(crate) unparsed_tail: View<'a>,
}

#[derive(Debug)]
pub(crate) struct UfrxOccurrence<'a> {
    /// `reference_id` of the external reference the occurrence instantiates.
    pub(crate) reference_id: u32,
    pub(crate) occurrence_id: u32,
    pub(crate) occurrence_key: u32,
    pub(crate) header_value: u32,
    pub(crate) title: Option<String>,
    pub(crate) header_padding_words: u8,
    /// First-section property `0x03`: the referenced document's identifier.
    pub(crate) prototype_document_id: Option<[u8; 16]>,
    /// First-section property `0x05`: the referenced file's name stem.
    pub(crate) component_name: Option<String>,
    pub(crate) source: View<'a>,
}

/// Values of the first occurrence property section that join an occurrence
/// to its external reference. A tag that repeats with different values is
/// ambiguous and is kept as `None`.
#[derive(Debug, Default)]
struct OccurrenceJoinProperties {
    document_id: PropertySlot<[u8; 16]>,
    component_name: PropertySlot<String>,
}

#[derive(Debug, Default)]
enum PropertySlot<T> {
    #[default]
    Empty,
    Value(T),
    Ambiguous,
}

impl<T: PartialEq> PropertySlot<T> {
    fn offer(&mut self, value: T) {
        *self = match std::mem::take(self) {
            Self::Empty => Self::Value(value),
            Self::Value(existing) if existing == value => Self::Value(existing),
            Self::Value(_) | Self::Ambiguous => Self::Ambiguous,
        };
    }

    fn into_value(self) -> Option<T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Empty | Self::Ambiguous => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct UfrxModelState<'a> {
    pub(crate) prefix: u8,
    pub(crate) name: String,
    pub(crate) state: [u16; 2],
    pub(crate) prefix_count: u32,
    pub(crate) parameters: Vec<UfrxModelStateParameter>,
    pub(crate) suffix: View<'a>,
}

/// One pre-schema-15 level-of-detail record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UfrxLevelOfDetail {
    pub(crate) prefix: u8,
    pub(crate) name: String,
    pub(crate) state: [u16; 2],
    pub(crate) members: Vec<UfrxLevelOfDetailMember>,
}

/// One occurrence entry of a level-of-detail record, keyed by its decimal
/// occurrence-id string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UfrxLevelOfDetailMember {
    pub(crate) occurrence: String,
    pub(crate) kind: UfrxLevelOfDetailMemberKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UfrxLevelOfDetailMemberKind {
    /// Kind 1: the entry carries no payload.
    Listed,
    /// Kind 2: the entry names a level of detail of the referenced
    /// subassembly.
    Nested {
        state: [u16; 2],
        level_of_detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UfrxRepresentationState {
    pub(crate) prefix: u16,
    /// The stored `(name, kind)` representation list of an assembly; a part stores none.
    pub(crate) design_views: Option<Vec<(String, String)>>,
    pub(crate) secondary_active_lod_state: [u16; 2],
    pub(crate) active_design_view: String,
    pub(crate) active_design_view_state: [u16; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UfrxModelStateParameter {
    pub(crate) name: String,
    pub(crate) tag: u8,
    pub(crate) kind: u16,
    pub(crate) state: u16,
    pub(crate) value: String,
    pub(crate) trailer: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InventorExternalReference {
    pub(crate) path: String,
    pub(crate) library_id: i32,
    pub(crate) library_name: String,
    pub(crate) display_name: String,
    pub(crate) state_groups: Vec<[u16; 3]>,
    pub(crate) state: [u16; 2],
    pub(crate) document_id: [u8; 16],
    pub(crate) database_id: [u8; 16],
    pub(crate) reference_id: u32,
    pub(crate) occurrence_count: u32,
    pub(crate) version: u32,
    pub(crate) flags: u32,
}

#[derive(Debug)]
pub(crate) struct InventorEmbeddedReference<'a> {
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
    pub(crate) source: View<'a>,
}

pub(crate) fn parse<'a>(
    ctx: &DecodeContext<'a>,
    snapshot: &CompoundSnapshot<'a>,
    document_kind: &DocumentKind,
) -> Result<UfrxState<'a>, CodecError> {
    let Some(stream) = snapshot.stream("UFRxDoc") else {
        return Ok(UfrxState::Absent);
    };
    let source = snapshot.open(ctx, stream)?;
    Ok(
        match parse_stream(ctx, source, stream.id(), document_kind) {
            Ok(document) => UfrxState::Parsed(Box::new(document)),
            Err(CodecError::NotImplemented(detail)) => {
                let (schema, section_versions) = parse_schema_table(source)?;
                UfrxState::Unsupported {
                    stream: stream.id(),
                    schema,
                    section_versions,
                    source,
                    detail,
                }
            }
            Err(error) => UfrxState::Malformed {
                stream: stream.id(),
                detail: crate::issue_detail(error)?,
            },
        },
    )
}

fn parse_stream<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    stream: CompoundStreamId,
    document_kind: &DocumentKind,
) -> Result<UfrxDocument<'a>, CodecError> {
    let mut declaration = Cursor::new(source);
    let schema = declaration.u16("schema")?;
    match parse_stream_grammar(ctx, source, stream, document_kind) {
        Ok(document) => Ok(document),
        Err(error) if !(11..=15).contains(&schema) => match error {
            CodecError::ResourceLimit(_) => Err(error),
            _ => Err(CodecError::malformed(format_args!(
                "UFRxDoc 11-15 grammar does not fit; foreign schema {schema} is the probable cause: {error}"
            ))),
        },
        Err(error) => Err(error),
    }
}

fn parse_stream_grammar<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    stream: CompoundStreamId,
    document_kind: &DocumentKind,
) -> Result<UfrxDocument<'a>, CodecError> {
    let mut cursor = Cursor::new(source);
    let schema = cursor.u16("schema")?;
    let section_count = cursor.count16("section-version count", 256)?;
    if section_count < 5 {
        return Err(CodecError::Malformed(
            "UFRxDoc section-version table is too short".into(),
        ));
    }
    let mut section_versions = Vec::new();
    for _ in 0..section_count {
        section_versions.push(cursor.u16("section version")?);
    }
    let save_version = cursor.array::<8>("save version")?;
    cursor.take(8, "save FILETIME")?;
    cursor.take(8, "secondary version")?;
    cursor.take(8, "secondary FILETIME")?;
    cursor.utf16(ctx, "comment", 65_536)?;
    cursor.take(8, "creation version")?;
    cursor.take(8, "creation FILETIME")?;
    cursor.take(8, "origin version")?;
    cursor.take(8, "origin FILETIME")?;
    cursor.take(16, "database revision id")?;
    cursor.u32("database revision state")?;
    cursor.take(16, "internal document id")?;
    let original_file_name = cursor.utf16(ctx, "original file name", 65_536)?;
    cursor.u16("original file-name state")?;

    let lod_toc_count = cursor.count32("LOD table count", 65_536)?;
    for _ in 0..lod_toc_count {
        cursor.u16("LOD entry kind")?;
        cursor.u16("LOD entry state")?;
        cursor.utf16(ctx, "LOD name", 65_536)?;
        cursor.take(2, "LOD state")?;
    }

    let pair_count = cursor.count32("header pair count", 65_536)?;
    for _ in 0..pair_count {
        cursor.utf16(ctx, "header pair key", 65_536)?;
        cursor.utf16(ctx, "header pair value", 65_536)?;
    }
    cursor.u16("active LOD state")?;
    let design_view_count = usize::from(cursor.u16("design-view count")?);
    if schema == 15 {
        match document_kind {
            DocumentKind::Assembly | DocumentKind::Part => {}
            DocumentKind::Drawing
            | DocumentKind::Presentation
            | DocumentKind::Mixed
            | DocumentKind::Unknown => {
                return Err(CodecError::NotImplemented(format!(
                    "UFRxDoc schema 15 {} header is not implemented",
                    document_kind.label()
                )));
            }
        }
    }
    let representation = if schema == 15 {
        let prefix = cursor.u16("representation prefix")?;
        let mut design_views = Vec::new();
        for _ in 0..design_view_count {
            design_views.push((
                cursor.utf16(ctx, "design view", 65_536)?,
                cursor.utf16(ctx, "design view kind", 65_536)?,
            ));
        }
        Some(UfrxRepresentationState {
            prefix,
            design_views: Some(design_views),
            secondary_active_lod_state: [
                cursor.u16("secondary active LOD state")?,
                cursor.u16("secondary active LOD state")?,
            ],
            active_design_view: cursor.utf16(ctx, "active design view", 65_536)?,
            active_design_view_state: [
                cursor.u16("active design-view state")?,
                cursor.u16("active design-view state")?,
            ],
        })
    } else {
        if section_versions[2] >= 12 {
            cursor.utf16(ctx, "active design view", 65_536)?;
        }
        if section_versions[2] >= 7 {
            cursor.take(4, "secondary active LOD state")?;
        }
        cursor.u16("legacy representation state")?;
        None
    };
    cursor.u32("header state 0")?;
    cursor.u16("header state 1")?;
    if schema == 15 {
        cursor.u32("header state 1 extension")?;
    }
    let invariant = cursor.u16("header invariant")?;
    if invariant != 1 {
        return Err(CodecError::malformed(format_args!(
            "UFRxDoc header invariant is {invariant}, expected 1"
        )));
    }
    cursor.u32("header state 2")?;
    cursor.u32("header state 3")?;
    if section_versions[2] >= 13 {
        cursor.take(32, "document subtype ids")?;
    }

    let lod_count = cursor.count32("LOD count", 65_536)?;
    let (model_states, levels_of_detail) = if schema == 15 {
        (
            parse_model_states(ctx, source, &mut cursor, lod_count, section_versions[2])?,
            Vec::new(),
        )
    } else {
        (
            Vec::new(),
            parse_levels_of_detail(ctx, &mut cursor, lod_count)?,
        )
    };

    let reference_count = cursor.count32("external-reference count", 1_000_000)?;
    let caption = cursor.utf16(ctx, "external-reference caption", 65_536)?;
    cursor.u32("external-reference table state")?;
    let mut references = Vec::new();
    for _ in 0..reference_count {
        let path = cursor.utf16(ctx, "external path", 65_536)?;
        let library_id = cursor.i32("reference library id")?;
        let library_name = cursor.utf16(ctx, "library name", 65_536)?;
        cursor.u16("reference library state")?;
        let display_prefix = cursor.peek_u32("reference display-name prefix")?;
        if display_prefix & 0xffff_0000 != 0 && cursor.u16("reference display-name padding")? != 0 {
            return Err(CodecError::Malformed(
                "UFRxDoc reference display-name padding is nonzero".into(),
            ));
        }
        let display_name = cursor.utf16(ctx, "reference display name", 65_536)?;
        let state_count = cursor.count32("reference state-group count", 65_536)?;
        let mut state_groups = Vec::new();
        for _ in 0..state_count {
            state_groups.push([
                cursor.u16("reference state-group entry")?,
                cursor.u16("reference state-group entry")?,
                cursor.u16("reference state-group entry")?,
            ]);
        }
        let state = [
            cursor.u16("reference state")?,
            cursor.u16("reference state")?,
        ];
        let document_id = cursor.array("referenced document id")?;
        let database_id = cursor.array("referenced database id")?;
        references.push(InventorExternalReference {
            path,
            library_id,
            library_name,
            display_name,
            state_groups,
            state,
            document_id,
            database_id,
            reference_id: cursor.u32("reference id")?,
            occurrence_count: cursor.u32("reference occurrence count")?,
            version: cursor.u32("reference version")?,
            flags: cursor.u32("reference flags")?,
        });
    }
    if section_versions[4] >= 2 && cursor.u8("external-reference terminator")? != 0 {
        return Err(CodecError::Malformed(
            "UFRxDoc external-reference terminator is nonzero".into(),
        ));
    }
    let embedded_references = parse_embedded_references(
        ctx,
        source,
        &mut cursor,
        section_versions.get(15).copied().unwrap_or_default(),
    )?;
    let occurrences = parse_occurrences(
        ctx,
        source,
        &mut cursor,
        section_versions[3],
        save_year(save_version[2]),
    )?;
    let unparsed_tail = source
        .child(source.start() + cursor.position(), source.end())
        .ok_or_else(|| CodecError::Malformed("UFRxDoc tail range is invalid".into()))?;
    Ok(UfrxDocument {
        stream,
        schema,
        section_versions,
        original_file_name,
        caption,
        representation,
        model_states,
        levels_of_detail,
        references,
        embedded_references,
        occurrences,
        unparsed_tail,
    })
}

fn save_year(major: u8) -> u16 {
    if major > 11 {
        u16::from(major) + 1996
    } else {
        u16::from(major)
    }
}

fn parse_embedded_references<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    cursor: &mut Cursor<'_>,
    section_version: u16,
) -> Result<Vec<InventorEmbeddedReference<'a>>, CodecError> {
    let count = cursor.count32("embedded-reference count", 1_000_000)?;
    let mut references = Vec::new();
    for _ in 0..count {
        let start = cursor.position();
        let value_0 = cursor.u32("embedded-reference value 0")?;
        let filetime = cursor.u64("embedded-reference FILETIME")?;
        let value_1 = cursor.u32("embedded-reference value 1")?;
        let extended_value = if section_version >= 7 {
            Some(cursor.u32("embedded-reference extended value")?)
        } else {
            None
        };
        let value_2 = cursor.u32("embedded-reference value 2")?;
        let path = cursor.utf16(ctx, "embedded-reference path", 65_536)?;
        let library_id = cursor.i32("embedded-reference library id")?;
        let library_name = cursor.utf16(ctx, "embedded-reference library name", 65_536)?;
        let state = cursor.u16("embedded-reference state")?;
        let display_name = cursor.utf16(ctx, "embedded-reference display name", 65_536)?;
        let state_values = cursor.array("embedded-reference state values")?;
        let record = source
            .child(source.start() + start, source.start() + cursor.position())
            .ok_or_else(|| {
                CodecError::Malformed("UFRxDoc embedded-reference range is invalid".into())
            })?;
        references.push(InventorEmbeddedReference {
            value_0,
            filetime,
            value_1,
            extended_value,
            value_2,
            path,
            library_id,
            library_name,
            state,
            display_name,
            state_values,
            source: record,
        });
    }
    if section_version >= 6 && cursor.u8("embedded-reference terminator")? != 0 {
        return Err(CodecError::Malformed(
            "UFRxDoc embedded-reference terminator is nonzero".into(),
        ));
    }
    Ok(references)
}

fn parse_occurrences<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    cursor: &mut Cursor<'_>,
    section_version: u16,
    save_year: u16,
) -> Result<Vec<UfrxOccurrence<'a>>, CodecError> {
    let count = cursor.count32("occurrence count", 1_000_000)?;
    let mut occurrences = Vec::new();
    require_u32(
        cursor.u32("occurrence table reserved value")?,
        0,
        "occurrence table reserved value",
    )?;
    for _ in 0..count {
        let start = cursor.position();
        let reference_id = cursor.u32("occurrence reference id")?;
        let occurrence_id = cursor.u32("occurrence id")?;
        let occurrence_key = cursor.u32("occurrence key")?;
        let header_value = cursor.u32("occurrence header value")?;
        let title_count = cursor.count32("occurrence title marker", 65_536)?;
        let title = if title_count == 0 {
            None
        } else if section_version >= 28 {
            Some(cursor.utf16(ctx, "occurrence title", 65_536)?)
        } else {
            Some(cursor.utf16_counted(ctx, "occurrence title", title_count)?)
        };
        let header_padding_words = if section_version >= 28 {
            let mut padding_words = 0_u8;
            while cursor.peek_u16("occurrence extended-header padding")? == 0 {
                if padding_words == 8 {
                    return Err(CodecError::Malformed(
                        "UFRxDoc occurrence extended-header padding exceeds eight words".into(),
                    ));
                }
                cursor.u16("occurrence extended-header padding")?;
                padding_words += 1;
            }
            let marker_offset = cursor.position();
            let marker = cursor.u16("occurrence extended-header marker")?;
            if marker != 0x2080 {
                return Err(CodecError::malformed(format_args!(
                    "UFRxDoc occurrence extended-header marker at offset {marker_offset} is {marker:#06x}, expected 0x2080"
                )));
            }
            require_u32(
                cursor.u32("occurrence extended-header state")?,
                0,
                "occurrence extended-header state",
            )?;
            require_u32(
                cursor.u32("occurrence extended-header state")?,
                1,
                "occurrence extended-header state",
            )?;
            require_u32(
                cursor.u32("occurrence extended-header state")?,
                0,
                "occurrence extended-header state",
            )?;
            padding_words
        } else {
            cursor.take(5, "occurrence header state")?;
            if section_version >= 20 {
                cursor.u8("occurrence header state")?;
            }
            if section_version >= 21 {
                cursor.u8("occurrence header state")?;
            }
            0
        };
        let mut join = OccurrenceJoinProperties::default();
        parse_occurrence_section(ctx, cursor, Some(&mut join))?;
        parse_occurrence_section(ctx, cursor, None)?;
        parse_occurrence_settings(ctx, cursor)?;
        parse_occurrence_export(ctx, cursor, save_year)?;
        let record = source
            .child(source.start() + start, source.start() + cursor.position())
            .ok_or_else(|| CodecError::Malformed("UFRxDoc occurrence range is invalid".into()))?;
        occurrences.push(UfrxOccurrence {
            reference_id,
            occurrence_id,
            occurrence_key,
            header_value,
            title,
            header_padding_words,
            prototype_document_id: join.document_id.into_value(),
            component_name: join.component_name.into_value(),
            source: record,
        });
    }
    Ok(occurrences)
}

fn parse_occurrence_section(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    mut join: Option<&mut OccurrenceJoinProperties>,
) -> Result<(), CodecError> {
    cursor.u32("occurrence section state")?;
    let count = cursor.count32("occurrence section property count", 65_536)?;
    for _ in 0..count {
        cursor.boolean("occurrence property presence")?;
        let tag = cursor.u8("occurrence property tag")?;
        cursor.u32("occurrence property state")?;
        require_tag(cursor.u8("occurrence property repeated tag")?, tag)?;
        let value = parse_occurrence_value(ctx, cursor, tag)?;
        if let Some(join) = join.as_deref_mut() {
            match (tag, value) {
                (0x03, OccurrenceValue::Id(id)) => join.document_id.offer(id),
                (0x05, OccurrenceValue::Text(name)) => join.component_name.offer(name),
                _ => {}
            }
        }
        cursor.u32("occurrence property trailer")?;
    }
    Ok(())
}

fn parse_occurrence_settings(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<(), CodecError> {
    let count = cursor.count32("occurrence setting count", 65_536)?;
    for _ in 0..count {
        cursor.utf16(ctx, "occurrence setting name", 65_536)?;
        cursor.take(16, "occurrence setting id")?;
        cursor.utf8("occurrence setting value", 65_536)?;
    }
    Ok(())
}

fn parse_occurrence_export(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    save_year: u16,
) -> Result<(), CodecError> {
    cursor.take(10, "occurrence export state")?;
    if (2015..2025).contains(&save_year) {
        cursor.u8("occurrence export padding")?;
    }
    let count = cursor.peek_u32("occurrence export count")?;
    let next = cursor.peek_u32_at(4, "occurrence export discriminator")?;
    if matches!(count, 0x00ff_ffff | u32::MAX) {
        cursor.u32("occurrence export sentinel")?;
        if cursor.u32("occurrence export sentinel trailer")? != 0 {
            return Err(CodecError::Malformed(
                "UFRxDoc occurrence export sentinel trailer is nonzero".into(),
            ));
        }
    } else if count > 1 || (count == 1 && next > 1) {
        if next > 0xffff {
            cursor.utf16(ctx, "occurrence export name", 65_536)?;
            cursor.take(16, "occurrence export id")?;
            cursor.utf8("occurrence export value", 65_536)?;
        } else {
            let count = cursor.count32("occurrence export count", 65_536)?;
            for _ in 0..count {
                cursor.utf16(ctx, "occurrence export name", 65_536)?;
                parse_occurrence_items(cursor)?;
                cursor.take(8, "occurrence export trailer")?;
                let linked = cursor.count32("occurrence export linked-name count", 65_536)?;
                for _ in 0..linked {
                    cursor.utf16(ctx, "occurrence export linked name", 65_536)?;
                }
                if save_year >= 2015 {
                    cursor.u8("occurrence export padding")?;
                }
            }
        }
    } else {
        cursor.u32("occurrence export empty state")?;
    }
    if save_year >= 2025 {
        cursor.u8("occurrence export padding")?;
        cursor.u32("occurrence export trailer")?;
    }
    Ok(())
}

fn parse_occurrence_items(cursor: &mut Cursor<'_>) -> Result<(), CodecError> {
    let count = cursor.count32("occurrence export item count", 65_536)?;
    let repeated = cursor.count32("occurrence export repeated item count", 65_536)?;
    if repeated != count {
        return Err(CodecError::malformed(format_args!(
            "UFRxDoc occurrence export item counts differ: {count} and {repeated}"
        )));
    }
    for _ in 0..count {
        cursor.boolean("occurrence export item presence")?;
        let tag = cursor.u8("occurrence export item tag")?;
        let value_count = cursor.count32("occurrence export item value count", 65_536)?;
        for _ in 0..value_count {
            require_tag(cursor.u8("occurrence export repeated tag")?, tag)?;
            parse_occurrence_item_value(cursor, tag)?;
        }
        cursor.u32("occurrence export item trailer")?;
    }
    Ok(())
}

/// A decoded occurrence property value; only the join tags are kept.
#[derive(Debug)]
enum OccurrenceValue {
    Text(String),
    Id([u8; 16]),
    Other,
}

fn parse_occurrence_value(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    tag: u8,
) -> Result<OccurrenceValue, CodecError> {
    match tag {
        0x05 => {
            return Ok(OccurrenceValue::Text(cursor.utf16(
                ctx,
                "occurrence property string",
                65_536,
            )?));
        }
        0x1e => {
            cursor.utf16(ctx, "occurrence property string", 65_536)?;
        }
        0x07 | 0x0d | 0x0f | 0x10 | 0x1d => {
            cursor.u8("occurrence property byte")?;
        }
        0x19 => {
            cursor.u32("occurrence property value")?;
        }
        0x03 => {
            return Ok(OccurrenceValue::Id(
                cursor.array("occurrence property document id")?,
            ));
        }
        0x02 | 0x11 | 0x12 | 0x13 | 0x15 | 0x16 | 0x17 | 0x18 | 0x1c | 0x1f | 0x20 | 0x22
        | 0x23 | 0x24 | 0x25 | 0x2a | 0x2b | 0x2c | 0x2d | 0x2e => {
            cursor.take(16, "occurrence property id")?;
        }
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "UFRxDoc occurrence property tag {tag:#04x} is not implemented"
            )));
        }
    }
    Ok(OccurrenceValue::Other)
}

fn parse_occurrence_item_value(cursor: &mut Cursor<'_>, tag: u8) -> Result<(), CodecError> {
    match tag {
        0x05 | 0x1e => {
            let units = cursor.count32("occurrence export item string", 65_536)?;
            cursor.take(units * 2, "occurrence export item string")?;
        }
        0x07 => {
            cursor.u8("occurrence export item byte")?;
        }
        0x19 => {
            cursor.u32("occurrence export item value")?;
        }
        0x12 | 0x16 | 0x17 | 0x18 | 0x1a | 0x23 | 0x24 | 0x25 | 0x2a => {
            cursor.take(16, "occurrence export item id")?;
        }
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "UFRxDoc occurrence export item tag {tag:#04x} is not implemented"
            )));
        }
    }
    Ok(())
}

fn require_tag(actual: u8, expected: u8) -> Result<(), CodecError> {
    if actual != expected {
        return Err(CodecError::malformed(format_args!(
            "UFRxDoc repeated occurrence tag is {actual:#04x}, expected {expected:#04x}"
        )));
    }
    Ok(())
}

fn require_u32(actual: u32, expected: u32, field: &'static str) -> Result<(), CodecError> {
    if actual != expected {
        return Err(CodecError::malformed(format_args!(
            "UFRxDoc {field} is {actual:#010x}, expected {expected:#010x}"
        )));
    }
    Ok(())
}

fn parse_model_states<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    cursor: &mut Cursor<'_>,
    count: usize,
    header_section_version: u16,
) -> Result<Vec<UfrxModelState<'a>>, CodecError> {
    let suffix_len = match header_section_version {
        _ if count == 0 => 0,
        0..=16 => 77,
        17 => {
            return Err(CodecError::NotImplemented(
                "UFRxDoc model-state suffix for header section version 17 is not implemented"
                    .into(),
            ))
        }
        _ => 81,
    };
    let mut states = Vec::new();
    for _ in 0..count {
        let prefix = cursor.u8("model-state prefix")?;
        let name = cursor.utf16(ctx, "model-state name", 65_536)?;
        let state = [
            cursor.u16("model-state state")?,
            cursor.u16("model-state state")?,
        ];
        let prefix_count = cursor.u32("model-state prefix count")?;
        let parameter_count = cursor.count32("model-state parameter count", 1_000_000)?;
        let mut parameters = Vec::new();
        for _ in 0..parameter_count {
            parameters.push(UfrxModelStateParameter {
                name: cursor.utf16(ctx, "model-state parameter name", 65_536)?,
                tag: cursor.u8("model-state parameter tag")?,
                kind: cursor.u16("model-state parameter kind")?,
                state: cursor.u16("model-state parameter state")?,
                value: cursor.utf16(ctx, "model-state parameter value", 65_536)?,
                trailer: cursor.u16("model-state parameter trailer")?,
            });
        }
        let suffix_start = cursor.position();
        cursor.take(suffix_len, "model-state suffix")?;
        let suffix = source
            .child(
                source.start() + suffix_start,
                source.start() + cursor.position(),
            )
            .ok_or_else(|| {
                CodecError::Malformed("UFRxDoc model-state suffix range is invalid".into())
            })?;
        states.push(UfrxModelState {
            prefix,
            name,
            state,
            prefix_count,
            parameters,
            suffix,
        });
    }
    Ok(states)
}

fn parse_levels_of_detail(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    count: usize,
) -> Result<Vec<UfrxLevelOfDetail>, CodecError> {
    let mut levels = Vec::new();
    for _ in 0..count {
        let prefix = cursor.u8("level-of-detail prefix")?;
        let name = cursor.utf16(ctx, "level-of-detail name", 65_536)?;
        let state = [
            cursor.u16("level-of-detail state")?,
            cursor.u16("level-of-detail state")?,
        ];
        let member_count = cursor.count32("level-of-detail member count", 1_000_000)?;
        let mut members = Vec::new();
        for _ in 0..member_count {
            let occurrence = cursor.utf16(ctx, "level-of-detail occurrence", 65_536)?;
            let kind = match cursor.u8("level-of-detail member kind")? {
                1 => UfrxLevelOfDetailMemberKind::Listed,
                2 => UfrxLevelOfDetailMemberKind::Nested {
                    state: [
                        cursor.u16("nested level-of-detail state")?,
                        cursor.u16("nested level-of-detail state")?,
                    ],
                    level_of_detail: cursor.utf16(ctx, "nested level-of-detail name", 65_536)?,
                },
                kind => {
                    return Err(CodecError::NotImplemented(format!(
                        "UFRxDoc level-of-detail member kind {kind} is not implemented"
                    )));
                }
            };
            if cursor.u8("level-of-detail member trailer")? != 0 {
                return Err(CodecError::Malformed(
                    "UFRxDoc level-of-detail member trailer is nonzero".into(),
                ));
            }
            members.push(UfrxLevelOfDetailMember { occurrence, kind });
        }
        if cursor.u8("level-of-detail terminator")? != 0 {
            return Err(CodecError::Malformed(
                "UFRxDoc level-of-detail terminator is nonzero".into(),
            ));
        }
        levels.push(UfrxLevelOfDetail {
            prefix,
            name,
            state,
            members,
        });
    }
    Ok(levels)
}

fn parse_schema_table(source: View<'_>) -> Result<(u16, Vec<u16>), CodecError> {
    let mut cursor = Cursor::new(source);
    let schema = cursor.u16("schema")?;
    let section_count = cursor.count16("section-version count", 256)?;
    let mut section_versions = Vec::new();
    for _ in 0..section_count {
        section_versions.push(cursor.u16("section version")?);
    }
    Ok((schema, section_versions))
}

struct Cursor<'a> {
    view: View<'a>,
}

impl<'a> Cursor<'a> {
    const fn new(view: View<'a>) -> Self {
        Self { view }
    }

    fn position(&self) -> usize {
        self.view.read_len()
    }

    fn take(&mut self, len: usize, field: &'static str) -> Result<&'a [u8], CodecError> {
        crate::reader::take(&mut self.view, len, field)
    }

    fn u8(&mut self, field: &'static str) -> Result<u8, CodecError> {
        crate::reader::u8(&mut self.view, field)
    }

    fn u16(&mut self, field: &'static str) -> Result<u16, CodecError> {
        crate::reader::u16(&mut self.view, field)
    }

    fn u32(&mut self, field: &'static str) -> Result<u32, CodecError> {
        crate::reader::u32(&mut self.view, field)
    }

    fn i32(&mut self, field: &'static str) -> Result<i32, CodecError> {
        crate::reader::i32(&mut self.view, field)
    }

    fn u64(&mut self, field: &'static str) -> Result<u64, CodecError> {
        crate::reader::u64(&mut self.view, field)
    }

    fn array<const N: usize>(&mut self, field: &'static str) -> Result<[u8; N], CodecError> {
        crate::reader::array(&mut self.view, field)
    }

    fn peek_u32(&self, field: &'static str) -> Result<u32, CodecError> {
        self.peek_u32_at(0, field)
    }

    fn peek_u16(&self, field: &'static str) -> Result<u16, CodecError> {
        let mut view = self.view;
        crate::reader::u16(&mut view, field)
    }

    fn peek_u32_at(&self, relative: usize, field: &'static str) -> Result<u32, CodecError> {
        let mut view = self.view;
        crate::reader::take(&mut view, relative, field)?;
        crate::reader::u32(&mut view, field)
    }

    fn count16(&mut self, field: &'static str, maximum: usize) -> Result<usize, CodecError> {
        let value = usize::from(self.u16(field)?);
        if value > maximum {
            return Err(CodecError::malformed(format_args!(
                "UFRxDoc {field} exceeds {maximum}"
            )));
        }
        Ok(value)
    }

    fn count32(&mut self, field: &'static str, maximum: usize) -> Result<usize, CodecError> {
        let offset = self.position();
        let value = usize::try_from(self.u32(field)?)
            .map_err(|_| CodecError::malformed(format_args!("UFRxDoc {field} is too large")))?;
        if value > maximum {
            return Err(CodecError::malformed(format_args!(
                "UFRxDoc {field} value {value} at offset {offset} exceeds {maximum}"
            )));
        }
        Ok(value)
    }

    fn utf16(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &'static str,
        maximum: usize,
    ) -> Result<String, CodecError> {
        let count = self.count32(field, maximum)?;
        self.utf16_counted(ctx, field, count)
    }

    fn utf16_counted(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &'static str,
        count: usize,
    ) -> Result<String, CodecError> {
        crate::reader::utf16_text(ctx, &mut self.view, count, field, "retain UFRxDoc string")
    }

    fn utf8(&mut self, field: &'static str, maximum: usize) -> Result<String, CodecError> {
        let count = self.count32(field, maximum)?;
        let value = self.take(count, field)?;
        std::str::from_utf8(value)
            .map(str::to_owned)
            .map_err(|_| CodecError::malformed(format_args!("UFRxDoc {field} is not UTF-8")))
    }

    fn boolean(&mut self, field: &'static str) -> Result<bool, CodecError> {
        match self.u8(field)? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(CodecError::malformed(format_args!(
                "UFRxDoc {field} is {value}, expected 0 or 1"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::test_fixtures::push_u16;
    use crate::test_support::test_fixtures::push_u32;
    use crate::test_support::test_fixtures::push_utf16;
    use cadmpeg_container::compound::CompoundStreamId;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    use super::{
        parse_embedded_references, parse_occurrence_item_value, parse_occurrence_value,
        parse_occurrences, parse_schema_table, parse_stream, Cursor, UfrxLevelOfDetail,
        UfrxLevelOfDetailMember, UfrxLevelOfDetailMemberKind, UfrxState,
    };
    use crate::rse::DocumentKind;
    use crate::test_support::truncation::{displayed_truncation, located_truncation};
    use cadmpeg_container::compound::CompoundSnapshot;
    use cadmpeg_core::decode::{DecodeContext, View};
    use cadmpeg_core::CodecError;

    #[test]
    fn ufrx_unknown_property_tags_are_not_implemented() {
        let arena = DecodeArena::new();
        let (service, root) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty root fits service policy");
        let property_error = parse_occurrence_value(&service, &mut Cursor::new(root), 0xff)
            .expect_err("unknown property tag rejected");
        let item_error = parse_occurrence_item_value(&mut Cursor::new(root), 0xff)
            .expect_err("unknown export tag rejected");
        assert!(property_error.to_string().contains("tag 0xff"));
        assert!(item_error.to_string().contains("tag 0xff"));
    }

    #[test]
    fn malformed_ufrx_remains_an_inventory_state() {
        let bytes = crate::test_support::test_fixtures::fixture_with_ufrx(&[0; 2]);
        let arena = DecodeArena::new();
        let (setup, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("compound input fits service policy");
        let snapshot = CompoundSnapshot::new(&setup, root).expect("synthetic compound parses");
        assert!(matches!(
            super::parse(&setup, &snapshot, &DocumentKind::Part)
                .expect("malformed UFRx remains an inventory state"),
            UfrxState::Malformed { .. }
        ));
    }

    #[test]
    fn utf16_string_decodes_counted_units() {
        let bytes = [b'A', 0];
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("build the decode context");
        assert_eq!(
            Cursor::new(root)
                .utf16_counted(&ctx, "value", 1)
                .expect("one UTF-16 unit"),
            "A"
        );
    }

    fn stream_id() -> CompoundStreamId {
        let bytes = crate::test_support::test_fixtures::fixture(true);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic compound file fits policy");
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("synthetic compound file parses");
        snapshot
            .stream("RSeStorage/RSeSegInfo")
            .expect("validated stream entry")
            .id()
    }

    fn assert_diagnostic_not_implemented(bytes: &[u8], document_kind: &DocumentKind) {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("build the decode context");
        assert!(matches!(
            parse_stream(&ctx, root, stream_id(), document_kind),
            Err(CodecError::NotImplemented(_))
        ));
    }

    #[test]
    fn schema_15_document_kind_is_not_implemented() {
        let (bytes, _) = fixture(15);
        assert_diagnostic_not_implemented(&bytes, &DocumentKind::Unknown);
    }

    #[test]
    fn pre_schema_15_levels_of_detail_are_framed() {
        let bytes = with_levels_of_detail(11, &level_of_detail_records(1));
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic UFRxDoc fits policy");
        let document = parse_stream(&ctx, root, stream_id(), &DocumentKind::Assembly)
            .expect("levels of detail frame");
        assert_eq!(
            document.levels_of_detail,
            vec![
                UfrxLevelOfDetail {
                    prefix: 2,
                    name: "Reduced".into(),
                    state: [2, 1],
                    members: vec![
                        UfrxLevelOfDetailMember {
                            occurrence: "20".into(),
                            kind: UfrxLevelOfDetailMemberKind::Listed,
                        },
                        UfrxLevelOfDetailMember {
                            occurrence: "21".into(),
                            kind: UfrxLevelOfDetailMemberKind::Nested {
                                state: [2, 0],
                                level_of_detail: "Master".into(),
                            },
                        },
                    ],
                },
                UfrxLevelOfDetail {
                    prefix: 2,
                    name: "Empty".into(),
                    state: [3, 1],
                    members: vec![],
                },
            ]
        );
        assert_eq!(document.references.len(), 1);
        assert_eq!(document.occurrences.len(), 1);
        assert_eq!(document.unparsed_tail.window(), b"tail");

        let bytes = with_levels_of_detail(11, &level_of_detail_records(9));
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic UFRxDoc fits policy");
        assert!(matches!(
            parse_stream(&ctx, root, stream_id(), &DocumentKind::Assembly),
            Err(CodecError::NotImplemented(detail)) if detail.contains("member kind 9")
        ));
    }

    /// Two level-of-detail records whose first member uses `first_kind`.
    fn level_of_detail_records(first_kind: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 2);
        bytes.push(2);
        push_utf16(&mut bytes, "Reduced");
        push_u16(&mut bytes, 2);
        push_u16(&mut bytes, 1);
        push_u32(&mut bytes, 2);
        push_utf16(&mut bytes, "20");
        bytes.push(first_kind);
        bytes.push(0);
        push_utf16(&mut bytes, "21");
        bytes.push(2);
        push_u16(&mut bytes, 2);
        push_u16(&mut bytes, 0);
        push_utf16(&mut bytes, "Master");
        bytes.push(0);
        bytes.push(0);
        bytes.push(2);
        push_utf16(&mut bytes, "Empty");
        push_u16(&mut bytes, 3);
        push_u16(&mut bytes, 1);
        push_u32(&mut bytes, 0);
        bytes.push(0);
        bytes
    }

    /// Replaces the empty LOD count of a pre-schema-15 fixture with `records`.
    fn with_levels_of_detail(schema: u16, records: &[u8]) -> Vec<u8> {
        let (mut bytes, invariant_offset) = fixture(schema);
        let lod_count_offset = invariant_offset + 2 + 4 + 4;
        bytes.splice(
            lod_count_offset..lod_count_offset + 4,
            records.iter().copied(),
        );
        bytes
    }

    #[test]
    fn supported_schemas_frame_external_references_and_retain_the_tail() {
        for schema in 11..=15 {
            let (bytes, _) = fixture(schema);
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("synthetic UFRxDoc fits policy");
            let document = parse_stream(&ctx, root, stream_id(), &DocumentKind::Assembly)
                .unwrap_or_else(|error| panic!("synthetic UFRxDoc schema {schema}: {error}"));
            assert_eq!(document.schema, schema);
            assert_eq!(document.original_file_name, "synthetic.ipt");
            assert_eq!(document.references.len(), 1);
            assert_eq!(document.references[0].path, "relative/component.ipt");
            assert_eq!(document.references[0].occurrence_count, 1);
            assert_eq!(document.occurrences.len(), 1);
            assert_eq!(document.occurrences[0].occurrence_id, 7);
            assert_eq!(document.occurrences[0].occurrence_key, 42);
            assert_eq!(document.occurrences[0].title.as_deref(), Some("placed"));
            if schema == 15 {
                let representation = document
                    .representation
                    .as_ref()
                    .expect("schema 15 representation state");
                assert_eq!(representation.active_design_view, "Master");
                assert_eq!(
                    representation.design_views,
                    Some(vec![("Default".to_owned(), "DesignView".to_owned())])
                );
                assert_eq!(document.model_states.len(), 1);
                assert_eq!(document.model_states[0].prefix, 0);
                assert_eq!(document.model_states[0].name, "Master");
                assert_eq!(document.model_states[0].parameters[0].name, "width");
                assert_eq!(document.model_states[0].parameters[0].value, "12.5");
                assert_eq!(document.model_states[0].suffix.window(), &[0x5a; 77]);
            } else {
                assert!(document.representation.is_none());
                assert!(document.model_states.is_empty());
            }
            assert_eq!(document.unparsed_tail.window(), b"tail");
        }
    }

    #[test]
    fn schema_15_part_stores_the_counted_design_view_list() {
        let (bytes, _) = fixture_for_kind(15, false);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic part UFRxDoc fits policy");

        let document = parse_stream(&ctx, root, stream_id(), &DocumentKind::Part)
            .expect("schema-15 part UFRxDoc parses");

        let representation = document.representation.expect("model-state header parses");
        assert_eq!(
            representation.design_views,
            Some(vec![
                ("Default".to_owned(), "DesignView".to_owned()),
                ("Shipping".to_owned(), "Export".to_owned()),
            ])
        );
        assert_eq!(representation.active_design_view, "Master");
        assert_eq!(document.model_states.len(), 1);
        assert_eq!(document.references.len(), 1);
    }

    #[test]
    fn a_foreign_schema_is_parsed_with_the_11_to_15_grammar() {
        let (bytes, _) = fixture(16);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic UFRxDoc fits policy");
        let document = parse_stream(&ctx, root, stream_id(), &DocumentKind::Assembly)
            .expect("foreign schema uses the residual grammar");
        assert_eq!(document.schema, 16);
        assert_eq!(document.original_file_name, "synthetic.ipt");
        assert_eq!(document.references.len(), 1);
    }

    #[test]
    fn a_broken_foreign_schema_names_the_schema_as_the_probable_cause() {
        let (mut bytes, invariant_offset) = fixture(16);
        bytes[invariant_offset..invariant_offset + 2].copy_from_slice(&2_u16.to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic UFRxDoc fits policy");
        let Err(error) = parse_stream(&ctx, root, stream_id(), &DocumentKind::Assembly) else {
            panic!("broken foreign schema must recover as unsupported");
        };
        assert!(
            matches!(&error, CodecError::Malformed(message)
                if message.contains("foreign schema 16 is the probable cause")
                    && !message.contains("schema 16 is not implemented")),
            "expected an attempted-grammar recovery, found {error:?}"
        );
    }

    #[test]
    fn schema_11_rejects_nonzero_header_invariant() {
        let (mut bytes, invariant_offset) = fixture(11);
        bytes[invariant_offset..invariant_offset + 2].copy_from_slice(&2_u16.to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic UFRxDoc fits policy");
        assert!(parse_stream(&ctx, root, stream_id(), &DocumentKind::Assembly,).is_err());
    }

    #[test]
    fn frames_extended_occurrence_header() {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 0);
        push_occurrence(&mut bytes, 28, 2020, 9, 17, "extended");
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic occurrence fits policy");
        let mut cursor = Cursor::new(root);

        let occurrences = parse_occurrences(&ctx, root, &mut cursor, 28, 2020)
            .expect("extended occurrence parses");

        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].occurrence_id, 9);
        assert_eq!(occurrences[0].occurrence_key, 17);
        assert_eq!(occurrences[0].title.as_deref(), Some("extended"));
        assert_eq!(occurrences[0].header_padding_words, 2);
        assert_eq!(cursor.position(), bytes.len());
    }

    /// One section-21 occurrence record with a raw title header, the two join
    /// properties in its first section and a one-entry export table.
    fn legacy_occurrence(reference_id: u32, title_header: &[u8], save_year: u16) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, reference_id);
        push_u32(&mut bytes, 9);
        push_u32(&mut bytes, 3);
        push_u32(&mut bytes, 1);
        bytes.extend_from_slice(title_header);
        bytes.extend_from_slice(&[0; 7]);
        push_u32(&mut bytes, 2);
        push_u32(&mut bytes, 2);
        bytes.extend_from_slice(&[1, 0x03]);
        push_u32(&mut bytes, 1);
        bytes.push(0x03);
        bytes.extend_from_slice(&[0x44; 16]);
        push_u32(&mut bytes, 0);
        bytes.extend_from_slice(&[1, 0x05]);
        push_u32(&mut bytes, 1);
        bytes.push(0x05);
        push_utf16(&mut bytes, "bracket");
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        bytes.extend_from_slice(&[0; 10]);
        if save_year >= 2015 {
            bytes.push(0);
        }
        push_u32(&mut bytes, 1);
        push_utf16(&mut bytes, "1:2");
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 1);
        bytes.extend_from_slice(&[1, 0x1a]);
        push_u32(&mut bytes, 1);
        bytes.push(0x1a);
        bytes.extend_from_slice(&[0x55; 16]);
        push_u32(&mut bytes, 0);
        bytes.extend_from_slice(&[0; 8]);
        push_u32(&mut bytes, 1);
        push_utf16(&mut bytes, "1:1");
        if save_year >= 2015 {
            bytes.push(0);
        }
        bytes
    }

    fn parse_legacy(bytes: &[u8], save_year: u16) -> Vec<(Option<String>, usize)> {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default())
            .expect("synthetic occurrence fits policy");
        let mut cursor = Cursor::new(root);
        let occurrences = parse_occurrences(&ctx, root, &mut cursor, 21, save_year)
            .expect("legacy occurrence parses");
        assert_eq!(cursor.position(), bytes.len());
        occurrences
            .iter()
            .map(|occurrence| {
                assert_eq!(occurrence.occurrence_id, 9);
                assert_eq!(occurrence.occurrence_key, 3);
                assert_eq!(occurrence.prototype_document_id, Some([0x44; 16]));
                assert_eq!(occurrence.component_name.as_deref(), Some("bracket"));
                (occurrence.title.clone(), occurrence.source.window().len())
            })
            .collect()
    }

    #[test]
    fn a_legacy_occurrence_frames_its_title_join_properties_and_export_links() {
        let mut counted = Vec::new();
        push_u32(&mut counted, 5);
        for unit in "Part4".encode_utf16() {
            push_u16(&mut counted, unit);
        }
        let bytes = legacy_occurrence(3, &counted, 2015);
        assert_eq!(
            parse_legacy(&bytes, 2015),
            vec![(Some("Part4".into()), bytes.len() - 8)]
        );

        // Before 2015 an export entry ends without the padding byte.
        let mut untitled = Vec::new();
        push_u32(&mut untitled, 0);
        for save_year in [2014, 2015] {
            let bytes = legacy_occurrence(3, &untitled, save_year);
            assert_eq!(
                parse_legacy(&bytes, save_year),
                vec![(None, bytes.len() - 8)]
            );
        }

        // The table's reserved value precedes the first record.
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 7);
        push_occurrence(&mut bytes, 18, 0, 7, 42, "placed");
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic occurrence fits policy");
        let mut cursor = Cursor::new(root);
        assert!(parse_occurrences(&ctx, root, &mut cursor, 18, 0).is_err());
    }

    #[test]
    fn frames_extended_embedded_reference() {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 3);
        bytes.extend_from_slice(&44_u64.to_le_bytes());
        push_u32(&mut bytes, 5);
        push_u32(&mut bytes, 6);
        push_u32(&mut bytes, 7);
        push_utf16(&mut bytes, "embedded/component.ipt");
        bytes.extend_from_slice(&(-2_i32).to_le_bytes());
        push_utf16(&mut bytes, "library");
        push_u16(&mut bytes, 8);
        push_utf16(&mut bytes, "component");
        bytes.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        bytes.push(0);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic embedded reference fits policy");
        let mut cursor = Cursor::new(root);

        let references = parse_embedded_references(&ctx, root, &mut cursor, 7)
            .expect("extended embedded reference parses");

        let [reference] = references.as_slice() else {
            panic!("one embedded reference must parse");
        };
        assert_eq!(reference.value_0, 3);
        assert_eq!(reference.filetime, 44);
        assert_eq!(reference.extended_value, Some(6));
        assert_eq!(reference.path, "embedded/component.ipt");
        assert_eq!(reference.state_values, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(reference.source.window().len() + 5, bytes.len());
        assert_eq!(cursor.position(), bytes.len());
    }

    fn fixture(schema: u16) -> (Vec<u8>, usize) {
        fixture_for_kind(schema, true)
    }

    fn fixture_for_kind(schema: u16, assembly: bool) -> (Vec<u8>, usize) {
        let mut bytes = Vec::new();
        push_u16(&mut bytes, schema);
        push_u16(&mut bytes, if schema == 15 { 27 } else { 23 });
        let header_section_version = if schema == 15 { 15 } else { 12 };
        for value in [
            31,
            19,
            header_section_version,
            18,
            1,
            2,
            4,
            2,
            1,
            3,
            1,
            2,
            6,
            2,
            2,
            5,
            0,
            1,
            2,
            0,
            0,
            0,
            0,
        ] {
            push_u16(&mut bytes, value);
        }
        if schema == 15 {
            for _ in 0..4 {
                push_u16(&mut bytes, 0);
            }
        }
        bytes.extend_from_slice(&[0; 32]);
        push_utf16(&mut bytes, "comment");
        bytes.extend_from_slice(&[0; 32]);
        bytes.extend_from_slice(&[0x10; 16]);
        push_u32(&mut bytes, 0);
        bytes.extend_from_slice(&[0x20; 16]);
        push_utf16(&mut bytes, "synthetic.ipt");
        push_u16(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        let design_view_count = match (schema, assembly) {
            (15, true) => 1,
            (15, false) => 2,
            _ => 0,
        };
        push_u16(&mut bytes, design_view_count);
        if schema == 15 {
            push_u16(&mut bytes, 0);
            push_utf16(&mut bytes, "Default");
            push_utf16(&mut bytes, "DesignView");
            if !assembly {
                push_utf16(&mut bytes, "Shipping");
                push_utf16(&mut bytes, "Export");
            }
            bytes.extend_from_slice(&[0; 4]);
            push_utf16(&mut bytes, "Master");
            bytes.extend_from_slice(&[0; 4]);
        } else {
            push_utf16(&mut bytes, "Default");
            bytes.extend_from_slice(&[0; 4]);
            push_u16(&mut bytes, 0);
        }
        push_u32(&mut bytes, 3);
        push_u16(&mut bytes, 0);
        if schema == 15 {
            push_u32(&mut bytes, 0);
        }
        let invariant_offset = bytes.len();
        push_u16(&mut bytes, 1);
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 0);
        if header_section_version >= 13 {
            bytes.extend_from_slice(&[0; 32]);
        }
        if schema == 15 {
            push_u32(&mut bytes, 1);
            bytes.push(0);
            push_utf16(&mut bytes, "Master");
            push_u16(&mut bytes, 2);
            push_u16(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 1);
            push_utf16(&mut bytes, "width");
            bytes.push(2);
            push_u16(&mut bytes, 0x48);
            push_u16(&mut bytes, 1);
            push_utf16(&mut bytes, "12.5");
            push_u16(&mut bytes, 0);
            bytes.extend_from_slice(&[0x5a; 77]);
        } else {
            push_u32(&mut bytes, 0);
        }
        push_u32(&mut bytes, 1);
        push_utf16(&mut bytes, "References");
        push_u32(&mut bytes, 0);
        push_utf16(&mut bytes, "relative/component.ipt");
        bytes.extend_from_slice(&(-1_i32).to_le_bytes());
        push_utf16(&mut bytes, "library");
        push_u16(&mut bytes, 0);
        push_utf16(&mut bytes, "component");
        push_u32(&mut bytes, 0);
        push_u16(&mut bytes, 1);
        push_u16(&mut bytes, 2);
        bytes.extend_from_slice(&[0x31; 16]);
        bytes.extend_from_slice(&[0x32; 16]);
        push_u32(&mut bytes, 7);
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 12);
        push_u32(&mut bytes, 4);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 0);
        push_occurrence(&mut bytes, 18, 0, 7, 42, "placed");
        bytes.extend_from_slice(b"tail");
        (bytes, invariant_offset)
    }

    fn push_occurrence(
        bytes: &mut Vec<u8>,
        section_version: u16,
        save_year: u16,
        occurrence_id: u32,
        occurrence_key: u32,
        title: &str,
    ) {
        push_u32(bytes, 1);
        push_u32(bytes, occurrence_id);
        push_u32(bytes, occurrence_key);
        push_u32(bytes, 0);
        if section_version >= 28 {
            push_u32(bytes, 1);
            push_utf16(bytes, title);
        } else {
            push_utf16(bytes, title);
        }
        if section_version >= 28 {
            push_u16(bytes, 0);
            push_u16(bytes, 0);
            push_u16(bytes, 0x2080);
            push_u32(bytes, 0);
            push_u32(bytes, 1);
            push_u32(bytes, 0);
        } else {
            bytes.extend_from_slice(&[0; 5]);
            if section_version >= 20 {
                bytes.push(0);
            }
            if section_version >= 21 {
                bytes.push(0);
            }
        }
        for _ in 0..2 {
            push_u32(bytes, 0);
            push_u32(bytes, 0);
        }
        push_u32(bytes, 0);
        bytes.extend_from_slice(&[0; 10]);
        if (2015..2025).contains(&save_year) {
            bytes.push(0);
        }
        push_u32(bytes, 0x00ff_ffff);
        push_u32(bytes, 0);
        if save_year >= 2025 {
            bytes.push(0);
            push_u32(bytes, 0);
        }
    }

    #[test]
    fn a_2025_export_section_moves_its_padding_to_the_end() {
        let mut bytes = Vec::new();
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 0);
        push_occurrence(&mut bytes, 28, 2025, 9, 17, "current");
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic occurrence fits policy");
        let mut cursor = Cursor::new(root);
        let occurrences =
            parse_occurrences(&ctx, root, &mut cursor, 28, 2025).expect("2025 occurrence parses");
        assert_eq!(occurrences.len(), 1);
        assert_eq!(cursor.position(), bytes.len());

        // The 2024 layout reads the padding byte where the 2025 sentinel starts.
        let mut cursor = Cursor::new(root);
        assert!(parse_occurrences(&ctx, root, &mut cursor, 28, 2024)
            .map_or(true, |_| cursor.position() != bytes.len()));
    }

    #[test]
    fn occurrence_property_tag_0x2e_holds_a_16_byte_identifier() {
        let mut bytes = vec![0x2e; 16];
        bytes.push(0xff);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic property fits policy");
        let mut cursor = Cursor::new(root);
        parse_occurrence_value(&ctx, &mut cursor, 0x2e).expect("tag 0x2e parses");
        assert_eq!(cursor.position(), 16);
    }

    /// One model state named "Master" with no parameters, a `suffix_len`-byte
    /// suffix, and four trailing bytes.
    fn model_state_record(suffix_len: usize) -> Vec<u8> {
        let mut bytes = vec![0];
        push_utf16(&mut bytes, "Master");
        push_u16(&mut bytes, 2);
        push_u16(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        bytes.extend(std::iter::repeat_n(0x5a, suffix_len));
        bytes.extend_from_slice(b"tail");
        bytes
    }

    #[test]
    fn model_state_suffix_length_follows_section_version_entry_2() {
        for (version, suffix_len) in [(16, 77), (18, 81), (19, 81)] {
            let bytes = model_state_record(suffix_len);
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("synthetic model state fits policy");
            let mut cursor = Cursor::new(root);
            let states = super::parse_model_states(&ctx, root, &mut cursor, 1, version)
                .expect("model state parses");
            assert_eq!(states[0].suffix.window().len(), suffix_len);
            assert_eq!(cursor.position(), bytes.len() - 4);
        }
        let bytes = model_state_record(81);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic model state fits policy");
        assert!(matches!(
            super::parse_model_states(&ctx, root, &mut Cursor::new(root), 1, 17),
            Err(CodecError::NotImplemented(_))
        ));
        let mut cursor = Cursor::new(root);
        assert!(super::parse_model_states(&ctx, root, &mut cursor, 0, 17)
            .expect("no model states at entry 2 value 17")
            .is_empty());
        assert_eq!(cursor.position(), 0);
    }

    #[test]
    fn truncated_ufrxdoc_scalar_reads_name_the_field() {
        let empty = &[];
        for (field, text) in [
            (
                "header state 1",
                displayed_truncation(Cursor::new(View::over_retained(empty)).u16("header state 1")),
            ),
            (
                "reference version",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).u32("reference version"),
                ),
            ),
            (
                "reference library id",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).i32("reference library id"),
                ),
            ),
            (
                "embedded-reference FILETIME",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).u64("embedded-reference FILETIME"),
                ),
            ),
        ] {
            assert_eq!(
                text,
                format!("truncated input during {field} at space 0 offset 0")
            );
        }
    }

    #[test]
    fn a_truncated_ufrxdoc_byte_read_is_located_and_names_its_field() {
        let empty: &[u8] = &[];
        for (field, text) in [
            (
                "reference kind",
                located_truncation(Cursor::new(View::over_retained(empty)).u8("reference kind")),
            ),
            (
                "referenced document id",
                located_truncation(
                    Cursor::new(View::over_retained(empty)).array::<16>("referenced document id"),
                ),
            ),
            (
                "reference name",
                located_truncation(
                    Cursor::new(View::over_retained(empty)).take(4, "reference name"),
                ),
            ),
            (
                "reference marker",
                located_truncation(
                    Cursor::new(View::over_retained(empty)).peek_u16("reference marker"),
                ),
            ),
            (
                "reference version",
                located_truncation(
                    Cursor::new(View::over_retained(empty)).peek_u32("reference version"),
                ),
            ),
        ] {
            assert_eq!(text, format!("Truncated {field} at offset 0"));
        }
    }

    #[test]
    fn a_truncated_ufrxdoc_utf16_string_is_located_and_names_its_field() {
        let bytes = [0x41, 0x00];
        let arena = DecodeArena::new();
        let text = match DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()) {
            Ok((ctx, root)) => {
                located_truncation(Cursor::new(root).utf16_counted(&ctx, "reference name", 4))
            }
            Err(error) => error.to_string(),
        };
        assert_eq!(text, "Truncated reference name at offset 0");
    }

    #[test]
    fn a_truncated_schema_table_names_the_field_it_stopped_in() {
        let bytes = 11_u16.to_le_bytes();
        let arena = DecodeArena::new();
        let text = match DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()) {
            Ok((_, root)) => displayed_truncation(parse_schema_table(root)),
            Err(error) => error.to_string(),
        };
        assert_eq!(
            text,
            "truncated input during section-version count at space 0 offset 2"
        );
    }
}
