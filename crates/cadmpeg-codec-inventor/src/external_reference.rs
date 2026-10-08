// SPDX-License-Identifier: Apache-2.0
//! Bounded `UFRxDoc` document and external-reference inventory.

use cadmpeg_container::compound::{CompoundSnapshot, CompoundStreamId};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::rse::DocumentKind;

// Each section version is one u16.
const MIN_SECTION_VERSION_BYTES: usize = 2;
// Two u16 states, one UTF-16 count word and one u16 trailer.
const MIN_LOD_TOC_BYTES: usize = 2 + 2 + 4 + 2;
// Two UTF-16 count words.
const MIN_HEADER_PAIR_BYTES: usize = 4 + 4;
// Three string counts, library id/state, group count, two state words,
// two 16-byte ids and four u32 values.
const MIN_REFERENCE_BYTES: usize = 3 * 4 + 4 + 2 + 4 + 2 * 2 + 2 * 16 + 4 * 4;
// Three u16 values.
const MIN_REFERENCE_STATE_GROUP_BYTES: usize = 3 * 2;
// Three u32 values, FILETIME, three string counts, library id, state and eight bytes.
const MIN_EMBEDDED_REFERENCE_BYTES: usize = 3 * 4 + 8 + 3 * 4 + 4 + 2 + 8;
// Four u32 values and title marker, five header bytes, two section state/count
// pairs, setting count, ten export bytes and an empty export count word.
const MIN_OCCURRENCE_BYTES: usize = 5 * 4 + 5 + 2 * (4 + 4) + 4 + 10 + 4;
// Presence, tag, state word and repeated tag identify the property before its
// value and trailer are read.
const MIN_OCCURRENCE_PROPERTY_HEADER_BYTES: usize = 1 + 1 + 4 + 1;
// Name count, sixteen-byte id and value count.
const MIN_OCCURRENCE_SETTING_BYTES: usize = 4 + 16 + 4;
// Name count, item count and repeated count, twelve-byte trailer.
const MIN_OCCURRENCE_EXPORT_BYTES: usize = 4 + 4 + 4 + 12;
// Presence, tag and value count identify an export item before its values and
// trailer are read.
const MIN_OCCURRENCE_ITEM_HEADER_BYTES: usize = 1 + 1 + 4;
// Each declared export value starts with a repeated tag.
const MIN_OCCURRENCE_VALUE_BYTES: usize = 1;
// Prefix, name count, two state words, prefix count, parameter count and suffix.
const MIN_MODEL_STATE_BYTES: usize = 1 + 4 + 2 * 2 + 4 + 4 + 77;
// Name count, tag, kind, state, value count and trailer.
const MIN_MODEL_STATE_PARAMETER_BYTES: usize = 4 + 1 + 2 + 2 + 4 + 2;

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
    pub(crate) references: Vec<InventorExternalReference>,
    pub(crate) embedded_references: Vec<InventorEmbeddedReference<'a>>,
    pub(crate) occurrences: Vec<UfrxOccurrence<'a>>,
    pub(crate) unparsed_tail: View<'a>,
}

#[derive(Debug)]
pub(crate) struct UfrxOccurrence<'a> {
    pub(crate) end_string_flag: u32,
    pub(crate) file_reference_id: u32,
    pub(crate) occurrence_id: u32,
    pub(crate) header_value: u32,
    pub(crate) title: Option<String>,
    pub(crate) header_padding_words: u8,
    pub(crate) source: View<'a>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UfrxRepresentationState {
    pub(crate) prefix: u16,
    pub(crate) active_representation: Option<(String, String)>,
    pub(crate) secondary_active_lod_state: [u16; 2],
    pub(crate) active_model_state: String,
    pub(crate) active_model_state_state: [u16; 2],
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
    let Some(stream) = snapshot.stream(ctx, "UFRxDoc")? else {
        return Ok(UfrxState::Absent);
    };
    let source = snapshot.open(ctx, stream)?;
    Ok(
        match parse_stream(ctx, source, stream.id(), document_kind) {
            Ok(document) => UfrxState::Parsed(Box::new(document)),
            Err(CodecError::NotImplemented(detail)) => {
                let (schema, section_versions) = parse_schema_table(ctx, source)?;
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
                detail: crate::issue_detail(ctx, error, "retain Inventor malformed UFRx detail")?,
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
    let assembly_representation = match (schema, document_kind) {
        (15, DocumentKind::Assembly) => Some(true),
        (15, DocumentKind::Part) => Some(false),
        (15, _) => None,
        _ => Some(false),
    };
    let section_count = cursor.count16("section-version count", 256)?;
    if section_count < 5 {
        return Err(CodecError::Malformed(
            "UFRxDoc section-version table is too short".into(),
        ));
    }
    cursor.fits(
        section_count,
        MIN_SECTION_VERSION_BYTES,
        "section-version count",
    )?;
    let section_versions = if assembly_representation.is_some() {
        let mut versions =
            ctx.vector_storage(section_count, "admit UFRxDoc section-version entries")?;
        let mut entries = 0..section_count;
        while ctx
            .next_charged(&mut entries, "admit UFRxDoc section-version entries")?
            .is_some()
        {
            ctx.push_vec(
                &mut versions,
                cursor.u16("section version")?,
                "admit UFRxDoc section-version entries",
            )?;
        }
        versions
    } else {
        // Every u16 value is valid. The unsupported header needs only bounds.
        cursor.take(
            section_count * MIN_SECTION_VERSION_BYTES,
            "section versions",
        )?;
        Vec::new()
    };
    let save_version = cursor.array::<8>("save version")?;
    cursor.take(8, "save FILETIME")?;
    cursor.take(8, "secondary version")?;
    cursor.take(8, "secondary FILETIME")?;
    cursor.skip_utf16(ctx, "comment", 65_536)?;
    cursor.take(8, "creation version")?;
    cursor.take(8, "creation FILETIME")?;
    cursor.take(8, "origin version")?;
    cursor.take(8, "origin FILETIME")?;
    cursor.take(16, "database revision id")?;
    cursor.u32("database revision state")?;
    cursor.take(16, "internal document id")?;
    let original_file_name = if assembly_representation.is_some() {
        cursor.utf16(ctx, "original file name", 65_536)?
    } else {
        cursor.skip_utf16(ctx, "original file name", 65_536)?;
        String::new()
    };
    cursor.u16("original file-name state")?;

    let lod_toc_count = cursor.count32("LOD table count", 65_536)?;
    cursor.fits(lod_toc_count, MIN_LOD_TOC_BYTES, "LOD table count")?;
    let mut lod_entries = 0..lod_toc_count;
    while ctx
        .next_charged(&mut lod_entries, "admit UFRxDoc LOD table entries")?
        .is_some()
    {
        cursor.u16("LOD entry kind")?;
        cursor.u16("LOD entry state")?;
        cursor.skip_utf16(ctx, "LOD name", 65_536)?;
        cursor.take(2, "LOD state")?;
    }

    let pair_count = cursor.count32("header pair count", 65_536)?;
    cursor.fits(pair_count, MIN_HEADER_PAIR_BYTES, "header pair count")?;
    let mut pairs = 0..pair_count;
    while ctx
        .next_charged(&mut pairs, "admit UFRxDoc header pairs")?
        .is_some()
    {
        cursor.skip_utf16(ctx, "header pair key", 65_536)?;
        cursor.skip_utf16(ctx, "header pair value", 65_536)?;
    }
    cursor.take(4, "active LOD state")?;
    let assembly_representation = assembly_representation.ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "UFRxDoc schema 15 {} header is not implemented",
            document_kind.label()
        ))
    })?;
    let representation = if schema == 15 {
        let prefix = cursor.u16("representation prefix")?;
        let active_representation = if assembly_representation {
            Some((
                cursor.utf16(ctx, "active representation", 65_536)?,
                cursor.utf16(ctx, "active representation kind", 65_536)?,
            ))
        } else {
            None
        };
        Some(UfrxRepresentationState {
            prefix,
            active_representation,
            secondary_active_lod_state: [
                cursor.u16("secondary active LOD state")?,
                cursor.u16("secondary active LOD state")?,
            ],
            active_model_state: cursor.utf16(ctx, "active model state", 65_536)?,
            active_model_state_state: [
                cursor.u16("active model-state state")?,
                cursor.u16("active model-state state")?,
            ],
        })
    } else {
        if section_versions[2] >= 12 {
            cursor.skip_utf16(ctx, "active design view", 65_536)?;
        }
        if section_versions[2] >= 7 {
            cursor.take(4, "secondary active LOD state")?;
        }
        cursor.u16("legacy representation state")?;
        None
    };
    cursor.u32("header state 0")?;
    cursor.u16("header state 1")?;
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
    let model_states = if schema == 15 {
        parse_model_states(ctx, source, &mut cursor, lod_count)?
    } else if lod_count != 0 {
        return Err(CodecError::NotImplemented(format!(
            "UFRxDoc contains {lod_count} unframed LOD records"
        )));
    } else {
        Vec::new()
    };

    let reference_count = cursor.count32("external-reference count", 1_000_000)?;
    let caption = cursor.utf16(ctx, "external-reference caption", 65_536)?;
    cursor.u32("external-reference table state")?;
    cursor.fits(
        reference_count,
        MIN_REFERENCE_BYTES,
        "external-reference count",
    )?;
    let mut references =
        ctx.vector_storage(reference_count, "admit Inventor external references")?;
    let mut reference_entries = 0..reference_count;
    while ctx
        .next_charged(&mut reference_entries, "admit Inventor external references")?
        .is_some()
    {
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
        cursor.fits(
            state_count,
            MIN_REFERENCE_STATE_GROUP_BYTES,
            "reference state-group count",
        )?;
        let mut state_groups = ctx.vector_storage(
            state_count,
            "admit Inventor external-reference state groups",
        )?;
        let mut state_group_entries = 0..state_count;
        while ctx
            .next_charged(
                &mut state_group_entries,
                "admit Inventor external-reference state groups",
            )?
            .is_some()
        {
            ctx.push_vec(
                &mut state_groups,
                [
                    cursor.u16("reference state-group entry")?,
                    cursor.u16("reference state-group entry")?,
                    cursor.u16("reference state-group entry")?,
                ],
                "admit Inventor external-reference state groups",
            )?;
        }
        let state = [
            cursor.u16("reference state")?,
            cursor.u16("reference state")?,
        ];
        let document_id = cursor.array("referenced document id")?;
        let database_id = cursor.array("referenced database id")?;
        ctx.push_vec(
            &mut references,
            InventorExternalReference {
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
            },
            "admit Inventor external references",
        )?;
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
    // Section version 7 adds one u32 extended value.
    let minimum = MIN_EMBEDDED_REFERENCE_BYTES + if section_version >= 7 { 4 } else { 0 };
    cursor.fits(count, minimum, "embedded-reference count")?;
    let mut references = ctx.vector_storage(count, "admit UFRxDoc embedded references")?;
    let mut entries = 0..count;
    while ctx
        .next_charged(&mut entries, "admit UFRxDoc embedded references")?
        .is_some()
    {
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
        ctx.push_vec(
            &mut references,
            InventorEmbeddedReference {
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
            },
            "admit UFRxDoc embedded references",
        )?;
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
    // The extended header replaces five bytes with a marker and three u32
    // states. Legacy versions 20 and 21 each add a byte; saves from 2015
    // add one export padding byte.
    let header_extra = if section_version >= 28 {
        9
    } else {
        usize::from(section_version >= 20) + usize::from(section_version >= 21)
    };
    let minimum = MIN_OCCURRENCE_BYTES + header_extra + usize::from(save_year >= 2015);
    cursor.fits(count, minimum, "occurrence count")?;
    let mut occurrences = ctx.vector_storage(count, "admit UFRxDoc occurrences")?;
    let mut entries = 0..count;
    while ctx
        .next_charged(&mut entries, "admit UFRxDoc occurrences")?
        .is_some()
    {
        let start = cursor.position();
        let end_string_flag = cursor.u32("occurrence end-string flag")?;
        let file_reference_id = cursor.u32("occurrence file-reference id")?;
        let occurrence_id = cursor.u32("occurrence id")?;
        let header_value = cursor.u32("occurrence header value")?;
        let title_count = cursor.count32("occurrence title marker", 65_536)?;
        let title = if title_count == 0 {
            None
        } else if section_version >= 28 || title_count == 1 {
            Some(cursor.utf16(ctx, "occurrence title", 65_536)?)
        } else {
            Some(cursor.utf16_counted(ctx, "occurrence title", title_count)?)
        };
        let header_padding_words = if section_version >= 28 {
            let mut padding_words = 0_u8;
            loop {
                if cursor.peek_u16("occurrence extended-header padding")? != 0 {
                    break;
                }
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
        parse_occurrence_section(ctx, cursor)?;
        parse_occurrence_section(ctx, cursor)?;
        parse_occurrence_settings(ctx, cursor)?;
        parse_occurrence_export(ctx, cursor, save_year)?;
        let record = source
            .child(source.start() + start, source.start() + cursor.position())
            .ok_or_else(|| CodecError::Malformed("UFRxDoc occurrence range is invalid".into()))?;
        ctx.push_vec(
            &mut occurrences,
            UfrxOccurrence {
                end_string_flag,
                file_reference_id,
                occurrence_id,
                header_value,
                title,
                header_padding_words,
                source: record,
            },
            "admit UFRxDoc occurrences",
        )?;
    }
    if count == 0 {
        cursor.u32("occurrence table terminator")?;
    }
    Ok(occurrences)
}

fn parse_occurrence_section(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<(), CodecError> {
    cursor.u32("occurrence section state")?;
    let count = cursor.count32("occurrence section property count", 65_536)?;
    cursor.fits(
        count,
        MIN_OCCURRENCE_PROPERTY_HEADER_BYTES,
        "occurrence section property count",
    )?;
    let mut properties = 0..count;
    while ctx
        .next_charged(&mut properties, "admit UFRxDoc occurrence properties")?
        .is_some()
    {
        cursor.boolean("occurrence property presence")?;
        let tag = cursor.u8("occurrence property tag")?;
        cursor.u32("occurrence property state")?;
        require_tag(cursor.u8("occurrence property repeated tag")?, tag)?;
        parse_occurrence_value(ctx, cursor, tag)?;
        cursor.u32("occurrence property trailer")?;
    }
    Ok(())
}

fn parse_occurrence_settings(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<(), CodecError> {
    let count = cursor.count32("occurrence setting count", 65_536)?;
    cursor.fits(
        count,
        MIN_OCCURRENCE_SETTING_BYTES,
        "occurrence setting count",
    )?;
    let mut settings = 0..count;
    while ctx
        .next_charged(&mut settings, "admit UFRxDoc occurrence settings")?
        .is_some()
    {
        cursor.skip_utf16(ctx, "occurrence setting name", 65_536)?;
        cursor.take(16, "occurrence setting id")?;
        cursor.skip_utf8(ctx, "occurrence setting value", 65_536)?;
    }
    Ok(())
}

fn parse_occurrence_export(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    save_year: u16,
) -> Result<(), CodecError> {
    cursor.take(10, "occurrence export state")?;
    if save_year >= 2015 {
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
            cursor.skip_utf16(ctx, "occurrence export name", 65_536)?;
            cursor.take(16, "occurrence export id")?;
            cursor.skip_utf8(ctx, "occurrence export value", 65_536)?;
        } else {
            let count = cursor.count32("occurrence export count", 65_536)?;
            let minimum = MIN_OCCURRENCE_EXPORT_BYTES + usize::from(save_year >= 2018);
            cursor.fits(count, minimum, "occurrence export count")?;
            let mut exports = 0..count;
            while ctx
                .next_charged(&mut exports, "admit UFRxDoc occurrence exports")?
                .is_some()
            {
                cursor.skip_utf16(ctx, "occurrence export name", 65_536)?;
                parse_occurrence_items(ctx, cursor)?;
                cursor.take(12, "occurrence export trailer")?;
                if save_year >= 2018 {
                    cursor.u8("occurrence export padding")?;
                }
            }
        }
    } else {
        cursor.u32("occurrence export empty state")?;
    }
    Ok(())
}

fn parse_occurrence_items(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<(), CodecError> {
    let count = cursor.count32("occurrence export item count", 65_536)?;
    let repeated = cursor.count32("occurrence export repeated item count", 65_536)?;
    if repeated != count {
        return Err(CodecError::malformed(format_args!(
            "UFRxDoc occurrence export item counts differ: {count} and {repeated}"
        )));
    }
    cursor.fits(
        count,
        MIN_OCCURRENCE_ITEM_HEADER_BYTES,
        "occurrence export item count",
    )?;
    let mut items = 0..count;
    while ctx
        .next_charged(&mut items, "admit UFRxDoc occurrence export items")?
        .is_some()
    {
        cursor.boolean("occurrence export item presence")?;
        let tag = cursor.u8("occurrence export item tag")?;
        let value_count = cursor.count32("occurrence export item value count", 65_536)?;
        cursor.fits(
            value_count,
            MIN_OCCURRENCE_VALUE_BYTES,
            "occurrence export item value count",
        )?;
        let mut values = 0..value_count;
        while ctx
            .next_charged(&mut values, "admit UFRxDoc occurrence export values")?
            .is_some()
        {
            require_tag(cursor.u8("occurrence export repeated tag")?, tag)?;
            parse_occurrence_item_value(cursor, tag)?;
        }
        cursor.u32("occurrence export item trailer")?;
    }
    Ok(())
}

fn parse_occurrence_value(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    tag: u8,
) -> Result<(), CodecError> {
    match tag {
        0x05 | 0x1e => {
            cursor.skip_utf16(ctx, "occurrence property string", 65_536)?;
        }
        0x07 | 0x0d | 0x0f | 0x10 | 0x1d => {
            cursor.u8("occurrence property byte")?;
        }
        0x19 => {
            cursor.u32("occurrence property value")?;
        }
        0x02 | 0x03 | 0x11 | 0x12 | 0x13 | 0x15 | 0x16 | 0x17 | 0x18 | 0x1c | 0x1f | 0x20
        | 0x22 | 0x23 | 0x24 | 0x25 | 0x2a | 0x2b | 0x2c | 0x2d => {
            cursor.take(16, "occurrence property id")?;
        }
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "UFRxDoc occurrence property tag {tag:#04x} is not implemented"
            )));
        }
    }
    Ok(())
}

fn parse_occurrence_item_value(cursor: &mut Cursor<'_>, tag: u8) -> Result<(), CodecError> {
    match tag {
        0x07 => {
            cursor.u8("occurrence export item byte")?;
        }
        0x19 => {
            cursor.u32("occurrence export item value")?;
        }
        0x12 | 0x16 | 0x17 | 0x18 | 0x23 | 0x24 | 0x25 | 0x2a => {
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
) -> Result<Vec<UfrxModelState<'a>>, CodecError> {
    cursor.fits(count, MIN_MODEL_STATE_BYTES, "model-state count")?;
    let mut states = ctx.vector_storage(count, "admit UFRxDoc model states")?;
    let mut state_entries = 0..count;
    while ctx
        .next_charged(&mut state_entries, "admit UFRxDoc model states")?
        .is_some()
    {
        let prefix = cursor.u8("model-state prefix")?;
        let name = cursor.utf16(ctx, "model-state name", 65_536)?;
        let state = [
            cursor.u16("model-state state")?,
            cursor.u16("model-state state")?,
        ];
        let prefix_count = cursor.u32("model-state prefix count")?;
        let parameter_count = cursor.count32("model-state parameter count", 1_000_000)?;
        cursor.fits(
            parameter_count,
            MIN_MODEL_STATE_PARAMETER_BYTES,
            "model-state parameter count",
        )?;
        let mut parameters =
            ctx.vector_storage(parameter_count, "admit UFRxDoc model-state parameters")?;
        let mut parameter_entries = 0..parameter_count;
        while ctx
            .next_charged(
                &mut parameter_entries,
                "admit UFRxDoc model-state parameters",
            )?
            .is_some()
        {
            ctx.push_vec(
                &mut parameters,
                UfrxModelStateParameter {
                    name: cursor.utf16(ctx, "model-state parameter name", 65_536)?,
                    tag: cursor.u8("model-state parameter tag")?,
                    kind: cursor.u16("model-state parameter kind")?,
                    state: cursor.u16("model-state parameter state")?,
                    value: cursor.utf16(ctx, "model-state parameter value", 65_536)?,
                    trailer: cursor.u16("model-state parameter trailer")?,
                },
                "admit UFRxDoc model-state parameters",
            )?;
        }
        let suffix_start = cursor.position();
        cursor.take(77, "model-state suffix")?;
        let suffix = source
            .child(
                source.start() + suffix_start,
                source.start() + cursor.position(),
            )
            .ok_or_else(|| {
                CodecError::Malformed("UFRxDoc model-state suffix range is invalid".into())
            })?;
        ctx.push_vec(
            &mut states,
            UfrxModelState {
                prefix,
                name,
                state,
                prefix_count,
                parameters,
                suffix,
            },
            "admit UFRxDoc model states",
        )?;
    }
    Ok(states)
}

fn parse_schema_table(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
) -> Result<(u16, Vec<u16>), CodecError> {
    let mut cursor = Cursor::new(source);
    let schema = cursor.u16("schema")?;
    let section_count = cursor.count16("section-version count", 256)?;
    cursor.fits(
        section_count,
        MIN_SECTION_VERSION_BYTES,
        "section-version count",
    )?;
    let mut section_versions =
        ctx.vector_storage(section_count, "admit UFRxDoc section-version entries")?;
    let mut entries = 0..section_count;
    while ctx
        .next_charged(&mut entries, "admit UFRxDoc section-version entries")?
        .is_some()
    {
        ctx.push_vec(
            &mut section_versions,
            cursor.u16("section version")?,
            "admit UFRxDoc section-version entries",
        )?;
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

    /// Checks the unread payload before admitting storage or traversal.
    fn fits(&self, count: usize, minimum: usize, field: &'static str) -> Result<(), CodecError> {
        self.view
            .counted(cadmpeg_core::decode::u64_from_index(count), minimum)
            .map(|_| ())
            .ok_or_else(|| {
                CodecError::malformed(format_args!("UFRxDoc {field} exceeds remaining payload"))
            })
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

    /// Validates a counted UTF-16 string the decode does not keep, without
    /// copying it; each decoded character is charged as it is read.
    fn skip_utf16(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &'static str,
        maximum: usize,
    ) -> Result<(), CodecError> {
        let count = self.count32(field, maximum)?;
        let len = count.checked_mul(2).ok_or_else(|| {
            CodecError::malformed(format_args!("UFRxDoc {field} length overflows"))
        })?;
        let bytes = self.take(len, field)?;
        let mut units = View::over_retained(bytes);
        let units = std::iter::from_fn(move || units.u16_le());
        if ctx.all_by(
            char::decode_utf16(units),
            |character| Ok(character.is_ok()),
            "validate unretained UFRxDoc string",
        )? {
            Ok(())
        } else {
            Err(CodecError::malformed(format_args!(
                "UFRxDoc {field} is not UTF-16"
            )))
        }
    }

    /// Validates a counted UTF-8 string the decode does not keep, without
    /// copying it.
    fn skip_utf8(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &'static str,
        maximum: usize,
    ) -> Result<(), CodecError> {
        let count = self.count32(field, maximum)?;
        let value = self.take(count, field)?;
        ctx.validate_utf8(value, "validate UFRxDoc UTF-8 string")?
            .map(|_| ())
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
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

    use super::{
        parse_embedded_references, parse_occurrence_item_value, parse_occurrence_value,
        parse_occurrences, parse_schema_table, parse_stream, Cursor, UfrxState,
    };
    use crate::rse::DocumentKind;

    use crate::test_support::truncation::{displayed_truncation, located_truncation};
    use cadmpeg_container::compound::CompoundSnapshot;
    use cadmpeg_core::decode::{DecodeContext, View};
    use cadmpeg_core::CodecError;

    fn assert_first_step_refusal(
        bytes: &[u8],
        operation: &'static str,
        parse: impl FnOnce(&DecodeContext<'_>, View<'_>) -> Result<(), CodecError>,
    ) {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("count fixture");
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, Some(1));
        let error = parse(&ctx, root).expect_err("the first loop step must be charged");
        drop(probe);
        assert!(
            matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == operation
                    && limit.additional == 1),
            "{error:?}"
        );
    }

    fn assert_impossible_count(
        bytes: &[u8],
        field: &str,
        visits: u64,
        parse: impl FnOnce(&DecodeContext<'_>, View<'_>) -> Result<(), CodecError>,
    ) {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        policy.limits.max_work_units = visits;
        let (ctx, root) =
            DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("count fixture");
        let error = parse(&ctx, root).expect_err("count does not fit the unread payload");
        assert!(
            matches!(error, CodecError::Malformed(ref detail)
            if detail == &format!("UFRxDoc {field} exceeds remaining payload")),
            "{error:?}"
        );
    }

    #[test]
    fn stream_counted_tables_charge_before_each_entry() {
        let (bytes, _) = fixture(11);
        let id = stream_id();
        assert_first_step_refusal(
            &bytes,
            "admit UFRxDoc section-version entries",
            |ctx, root| parse_stream(ctx, root, id, &DocumentKind::Assembly).map(|_| ()),
        );

        let lod_table_offset = 4 + 46 + 32 + 18 + 32 + 36 + 30 + 2;
        let mut bytes = bytes.clone();
        bytes[lod_table_offset..lod_table_offset + 4].copy_from_slice(&2_u32.to_le_bytes());
        let mut lod_entries = Vec::new();
        for _ in 0..2 {
            push_u16(&mut lod_entries, 0);
            push_u16(&mut lod_entries, 0);
            push_u32(&mut lod_entries, 0);
            push_u16(&mut lod_entries, 0);
        }
        drop(bytes.splice(lod_table_offset + 4..lod_table_offset + 4, lod_entries));
        assert_first_step_refusal(&bytes, "admit UFRxDoc LOD table entries", |ctx, root| {
            parse_stream(ctx, root, id, &DocumentKind::Assembly).map(|_| ())
        });

        let (mut bytes, _) = fixture(11);
        let pair_count_offset = lod_table_offset + 4;
        bytes[pair_count_offset..pair_count_offset + 4].copy_from_slice(&2_u32.to_le_bytes());
        let mut pairs = Vec::new();
        for _ in 0..2 {
            push_u32(&mut pairs, 0);
            push_u32(&mut pairs, 0);
        }
        drop(bytes.splice(pair_count_offset + 4..pair_count_offset + 4, pairs));
        assert_first_step_refusal(&bytes, "admit UFRxDoc header pairs", |ctx, root| {
            parse_stream(ctx, root, id, &DocumentKind::Assembly).map(|_| ())
        });

        let (mut bytes, invariant_offset) = fixture(11);
        let reference_count_offset = invariant_offset + 2 + 4 + 4 + 4;
        bytes[reference_count_offset..reference_count_offset + 4]
            .copy_from_slice(&2_u32.to_le_bytes());
        bytes.resize(bytes.len() + 2 * super::MIN_REFERENCE_BYTES, 0);
        assert_first_step_refusal(&bytes, "admit Inventor external references", |ctx, root| {
            parse_stream(ctx, root, id, &DocumentKind::Assembly).map(|_| ())
        });

        let (mut bytes, invariant_offset) = fixture(11);
        let state_group_count_offset =
            invariant_offset + 2 + 4 + 4 + 4 + 4 + 24 + 4 + 48 + 4 + 18 + 2 + 22;
        bytes[state_group_count_offset..state_group_count_offset + 4]
            .copy_from_slice(&2_u32.to_le_bytes());
        assert_first_step_refusal(
            &bytes,
            "admit Inventor external-reference state groups",
            |ctx, root| parse_stream(ctx, root, id, &DocumentKind::Assembly).map(|_| ()),
        );
    }

    #[test]
    fn nested_counted_records_charge_before_each_entry() {
        let mut bytes = Vec::new();
        push_u16(&mut bytes, 11);
        push_u16(&mut bytes, 2);
        bytes.extend_from_slice(&[0; 4]);
        assert_first_step_refusal(
            &bytes,
            "admit UFRxDoc section-version entries",
            |ctx, root| parse_schema_table(ctx, root).map(|_| ()),
        );

        let mut bytes = 2_u32.to_le_bytes().to_vec();
        bytes.resize(4 + 2 * super::MIN_EMBEDDED_REFERENCE_BYTES, 0);
        assert_first_step_refusal(&bytes, "admit UFRxDoc embedded references", |ctx, root| {
            parse_embedded_references(ctx, root, &mut Cursor::new(root), 0).map(|_| ())
        });

        let mut bytes = 2_u32.to_le_bytes().to_vec();
        bytes.resize(4 + 2 * super::MIN_OCCURRENCE_BYTES, 0);
        assert_first_step_refusal(&bytes, "admit UFRxDoc occurrences", |ctx, root| {
            parse_occurrences(ctx, root, &mut Cursor::new(root), 0, 0).map(|_| ())
        });

        let mut bytes = vec![0; 4];
        push_u32(&mut bytes, 2);
        bytes.resize(8 + 2 * super::MIN_OCCURRENCE_PROPERTY_HEADER_BYTES, 0);
        assert_first_step_refusal(
            &bytes,
            "admit UFRxDoc occurrence properties",
            |ctx, root| super::parse_occurrence_section(ctx, &mut Cursor::new(root)),
        );

        let mut bytes = 2_u32.to_le_bytes().to_vec();
        bytes.resize(4 + 2 * super::MIN_OCCURRENCE_SETTING_BYTES, 0);
        assert_first_step_refusal(&bytes, "admit UFRxDoc occurrence settings", |ctx, root| {
            super::parse_occurrence_settings(ctx, &mut Cursor::new(root))
        });

        let mut bytes = vec![0; 10];
        push_u32(&mut bytes, 2);
        push_u32(&mut bytes, 0);
        bytes.resize(18 + 2 * super::MIN_OCCURRENCE_EXPORT_BYTES, 0);
        assert_first_step_refusal(&bytes, "admit UFRxDoc occurrence exports", |ctx, root| {
            super::parse_occurrence_export(ctx, &mut Cursor::new(root), 0)
        });

        let mut bytes = 2_u32.to_le_bytes().to_vec();
        push_u32(&mut bytes, 2);
        bytes.resize(8 + 2 * super::MIN_OCCURRENCE_ITEM_HEADER_BYTES, 0);
        assert_first_step_refusal(
            &bytes,
            "admit UFRxDoc occurrence export items",
            |ctx, root| super::parse_occurrence_items(ctx, &mut Cursor::new(root)),
        );

        let mut bytes = 1_u32.to_le_bytes().to_vec();
        push_u32(&mut bytes, 1);
        bytes.extend_from_slice(&[0, 0x07]);
        push_u32(&mut bytes, 2);
        bytes.extend_from_slice(&[0x07, 1, 0x07, 2]);
        push_u32(&mut bytes, 0);
        assert_first_step_refusal(
            &bytes,
            "admit UFRxDoc occurrence export values",
            |ctx, root| super::parse_occurrence_items(ctx, &mut Cursor::new(root)),
        );

        let bytes = vec![0; 2 * super::MIN_MODEL_STATE_BYTES];
        assert_first_step_refusal(&bytes, "admit UFRxDoc model states", |ctx, root| {
            super::parse_model_states(ctx, root, &mut Cursor::new(root), 2).map(|_| ())
        });

        let mut bytes = vec![0; 1 + 4 + 4 + 4];
        push_u32(&mut bytes, 2);
        bytes.resize(
            bytes.len() + 2 * super::MIN_MODEL_STATE_PARAMETER_BYTES + 77,
            0,
        );
        assert_first_step_refusal(
            &bytes,
            "admit UFRxDoc model-state parameters",
            |ctx, root| super::parse_model_states(ctx, root, &mut Cursor::new(root), 1).map(|_| ()),
        );
    }

    #[test]
    fn completed_count_loop_pays_for_its_end_probe() {
        let mut bytes = Vec::new();
        push_u16(&mut bytes, 11);
        push_u16(&mut bytes, 5);
        bytes.extend_from_slice(&[0; 10]);

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 5;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("schema table");
        assert!(matches!(
            parse_schema_table(&ctx, root),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "admit UFRxDoc section-version entries"
                    && limit.used == 5
                    && limit.additional == 1
        ));

        policy.limits.max_work_units = 6;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("exact budget");
        let (schema, versions) = parse_schema_table(&ctx, root).expect("five entries and end");
        assert_eq!(schema, 11);
        assert_eq!(versions, vec![0; 5]);
        assert!(ctx.resource_refusal().is_none());
        let original = match ctx.charge_work(1, "probe completed count loop") {
            Err(CodecError::ResourceLimit(limit)) => limit,
            other => panic!("one extra work unit must refuse: {other:?}"),
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.used, 6);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
        );
    }

    #[test]
    fn a_supported_empty_count_still_pays_for_the_end_probe() {
        let bytes = 0_u32.to_le_bytes();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("empty setting list");
        assert!(matches!(
            super::parse_occurrence_settings(&ctx, &mut Cursor::new(root)),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "admit UFRxDoc occurrence settings"
                    && limit.used == 0
                    && limit.additional == 1
        ));

        policy.limits.max_work_units = 1;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("one work unit");
        super::parse_occurrence_settings(&ctx, &mut Cursor::new(root))
            .expect("empty settings are supported");
        assert!(ctx.resource_refusal().is_none());
        let original = match ctx.charge_work(1, "probe empty count loop") {
            Err(CodecError::ResourceLimit(limit)) => limit,
            other => panic!("one extra work unit must refuse: {other:?}"),
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.used, 1);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
        );
    }

    #[test]
    fn an_unsupported_first_property_does_not_precharge_unread_properties() {
        let mut bytes = vec![0; 4];
        push_u32(&mut bytes, 2);
        bytes.extend_from_slice(&[0, 0xff]);
        push_u32(&mut bytes, 0);
        bytes.push(0xff);
        bytes.extend_from_slice(&[0; super::MIN_OCCURRENCE_PROPERTY_HEADER_BYTES]);

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("property headers");
        let probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "admit UFRxDoc occurrence properties",
            Some(2),
        );
        let error = super::parse_occurrence_section(&ctx, &mut Cursor::new(root))
            .expect_err("the first property tag is unsupported");
        drop(probe);
        assert!(matches!(
            error,
            CodecError::NotImplemented(detail)
                if detail == "UFRxDoc occurrence property tag 0xff is not implemented"
        ));
        ctx.finish_session()
            .expect("unsupported dispatch did not refuse work");
    }

    #[test]
    fn a_bad_first_external_path_does_not_precharge_the_declared_tail() {
        let (mut bytes, invariant_offset) = fixture(11);
        let reference_count_offset = invariant_offset + 2 + 4 + 4 + 4;
        bytes[reference_count_offset..reference_count_offset + 4]
            .copy_from_slice(&2_u32.to_le_bytes());
        let path_start = reference_count_offset + 4 + 24 + 4;
        bytes[path_start + 4..path_start + 6].copy_from_slice(&0xd800_u16.to_le_bytes());
        bytes.resize(bytes.len() + 2 * super::MIN_REFERENCE_BYTES, 0);

        let id = stream_id();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("reference table");
        let probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "admit Inventor external references",
            Some(2),
        );
        let error = parse_stream(&ctx, root, id, &DocumentKind::Assembly)
            .expect_err("the first reference path is malformed");
        drop(probe);
        assert!(matches!(
            error,
            CodecError::Malformed(detail) if detail == "invalid UTF-16LE surrogate sequence"
        ));
        ctx.finish_session()
            .expect("the malformed first path did not refuse work");
    }

    #[test]
    fn schema_table_rejects_impossible_ufrx_count() {
        let mut bytes = Vec::new();
        push_u16(&mut bytes, 11);
        push_u16(&mut bytes, 5);
        bytes.extend_from_slice(&[0; 9]);
        assert_impossible_count(&bytes, "section-version count", 0, |ctx, root| {
            parse_schema_table(ctx, root).map(|_| ())
        });
    }

    #[test]
    fn embedded_references_reject_impossible_ufrx_count() {
        for (version, minimum) in [
            (0, super::MIN_EMBEDDED_REFERENCE_BYTES),
            (7, super::MIN_EMBEDDED_REFERENCE_BYTES + 4),
        ] {
            let mut bytes = 1_u32.to_le_bytes().to_vec();
            bytes.resize(4 + minimum - 1, 0);
            assert_impossible_count(&bytes, "embedded-reference count", 0, |ctx, root| {
                parse_embedded_references(ctx, root, &mut Cursor::new(root), version).map(|_| ())
            });
        }
    }

    #[test]
    fn occurrences_reject_impossible_ufrx_count() {
        for (version, year, extra) in [(0, 0, 0), (20, 2015, 2), (21, 2015, 3), (28, 2015, 10)] {
            let mut bytes = 1_u32.to_le_bytes().to_vec();
            bytes.resize(4 + super::MIN_OCCURRENCE_BYTES + extra - 1, 0);
            assert_impossible_count(&bytes, "occurrence count", 0, |ctx, root| {
                parse_occurrences(ctx, root, &mut Cursor::new(root), version, year).map(|_| ())
            });
        }
    }

    #[test]
    fn occurrence_property_tag_is_reported_before_missing_value_and_trailer() {
        let mut bytes = vec![0; 4];
        push_u32(&mut bytes, 1);
        bytes.resize(8 + 11, 0);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("property header context");
        assert!(matches!(
            super::parse_occurrence_section(&ctx, &mut Cursor::new(root)),
            Err(CodecError::NotImplemented(detail))
                if detail == "UFRxDoc occurrence property tag 0x00 is not implemented"
        ));
    }

    #[test]
    fn occurrence_properties_reject_count_with_truncated_header() {
        let mut bytes = vec![0; 4];
        push_u32(&mut bytes, 1);
        bytes.resize(8 + super::MIN_OCCURRENCE_PROPERTY_HEADER_BYTES - 1, 0);
        assert_impossible_count(
            &bytes,
            "occurrence section property count",
            0,
            |ctx, root| super::parse_occurrence_section(ctx, &mut Cursor::new(root)),
        );
    }

    #[test]
    fn occurrence_property_unsupported_tag_survives_truncated_record() {
        let mut bytes = vec![0; 4];
        push_u32(&mut bytes, 1);
        bytes.extend_from_slice(&[0, 0xff]);
        push_u32(&mut bytes, 0);
        bytes.push(0xff);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("unsupported property context");
        assert!(matches!(
            super::parse_occurrence_section(&ctx, &mut Cursor::new(root)),
            Err(CodecError::NotImplemented(detail))
                if detail == "UFRxDoc occurrence property tag 0xff is not implemented"
        ));
    }

    #[test]
    fn occurrence_settings_reject_impossible_ufrx_count() {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        bytes.resize(4 + super::MIN_OCCURRENCE_SETTING_BYTES - 1, 0);
        assert_impossible_count(&bytes, "occurrence setting count", 0, |ctx, root| {
            super::parse_occurrence_settings(ctx, &mut Cursor::new(root))
        });
    }

    #[test]
    fn occurrence_exports_reject_impossible_ufrx_count() {
        for (year, prefix, minimum) in [
            (0, 10, super::MIN_OCCURRENCE_EXPORT_BYTES),
            (2018, 11, super::MIN_OCCURRENCE_EXPORT_BYTES + 1),
        ] {
            let mut bytes = vec![0; prefix];
            push_u32(&mut bytes, 2);
            bytes.resize(prefix + 4 + 2 * minimum - 1, 0);
            assert_impossible_count(&bytes, "occurrence export count", 0, |ctx, root| {
                super::parse_occurrence_export(ctx, &mut Cursor::new(root), year)
            });
        }
    }

    #[test]
    fn occurrence_export_item_truncated_trailer_remains_truncated() {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        push_u32(&mut bytes, 1);
        bytes.resize(8 + 9, 0);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("item header context");
        assert!(matches!(
            super::parse_occurrence_items(&ctx, &mut Cursor::new(root)),
            Err(CodecError::Truncated {
                operation: "occurrence export item trailer",
                ..
            })
        ));
    }

    #[test]
    fn occurrence_items_reject_count_with_truncated_header() {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        push_u32(&mut bytes, 1);
        bytes.resize(8 + super::MIN_OCCURRENCE_ITEM_HEADER_BYTES - 1, 0);
        assert_impossible_count(&bytes, "occurrence export item count", 0, |ctx, root| {
            super::parse_occurrence_items(ctx, &mut Cursor::new(root))
        });
    }

    #[test]
    fn occurrence_export_unknown_tag_with_no_values_is_accepted() {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        push_u32(&mut bytes, 1);
        bytes.extend_from_slice(&[0, 0xff]);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("unsupported export item context");
        super::parse_occurrence_items(&ctx, &mut Cursor::new(root))
            .expect("an empty unsupported item has no value tag to reject");
    }

    #[test]
    fn occurrence_export_unsupported_tag_is_reported_after_matching_repeated_tag() {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        push_u32(&mut bytes, 1);
        bytes.extend_from_slice(&[0, 0xff]);
        push_u32(&mut bytes, 1);
        bytes.push(0xff);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("unsupported export item context");
        assert!(matches!(
            super::parse_occurrence_items(&ctx, &mut Cursor::new(root)),
            Err(CodecError::NotImplemented(detail))
                if detail == "UFRxDoc occurrence export item tag 0xff is not implemented"
        ));
    }

    #[test]
    fn occurrence_export_tag_mismatch_precedes_unsupported_dispatch() {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        push_u32(&mut bytes, 1);
        bytes.extend_from_slice(&[0, 0xff]);
        push_u32(&mut bytes, 1);
        bytes.push(0);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("mismatched export item context");
        assert!(matches!(
            super::parse_occurrence_items(&ctx, &mut Cursor::new(root)),
            Err(CodecError::Malformed(detail))
                if detail == "UFRxDoc repeated occurrence tag is 0x00, expected 0xff"
        ));
    }

    #[test]
    fn occurrence_export_unknown_tag_with_missing_repeated_tag_is_malformed() {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        push_u32(&mut bytes, 1);
        bytes.extend_from_slice(&[0, 0xff]);
        push_u32(&mut bytes, 1);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("truncated export item context");
        assert!(matches!(
            super::parse_occurrence_items(&ctx, &mut Cursor::new(root)),
            Err(CodecError::Malformed(detail))
                if detail == "UFRxDoc occurrence export item value count exceeds remaining payload"
        ));
    }

    #[test]
    fn occurrence_values_check_repeated_tag_before_payload_dispatch() {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        push_u32(&mut bytes, 1);
        bytes.extend_from_slice(&[0, 7]);
        push_u32(&mut bytes, 3);
        bytes.extend_from_slice(&[0; 4]);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("export value context");
        assert!(matches!(
            super::parse_occurrence_items(&ctx, &mut Cursor::new(root)),
            Err(CodecError::Malformed(detail))
                if detail == "UFRxDoc repeated occurrence tag is 0x00, expected 0x07"
        ));
    }

    #[test]
    fn occurrence_values_reject_count_without_repeated_tags() {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        push_u32(&mut bytes, 1);
        bytes.extend_from_slice(&[0, 7]);
        push_u32(&mut bytes, 3);
        // The outer item is admitted; three repeated tags need three bytes.
        assert_impossible_count(
            &bytes,
            "occurrence export item value count",
            1,
            |ctx, root| super::parse_occurrence_items(ctx, &mut Cursor::new(root)),
        );
    }

    #[test]
    fn model_states_reject_impossible_ufrx_count() {
        let bytes = vec![0; super::MIN_MODEL_STATE_BYTES - 1];
        assert_impossible_count(&bytes, "model-state count", 0, |ctx, root| {
            super::parse_model_states(ctx, root, &mut Cursor::new(root), 1).map(|_| ())
        });
    }

    #[test]
    fn model_parameters_reject_impossible_ufrx_count() {
        let mut bytes = vec![0; 1 + 4 + 4 + 4];
        push_u32(&mut bytes, 6);
        bytes.extend_from_slice(&[0; 77]);
        // One model-state visit reaches six parameters. Their 15-byte minima
        // need 90 bytes; only the 77-byte suffix remains.
        assert_impossible_count(&bytes, "model-state parameter count", 1, |ctx, root| {
            super::parse_model_states(ctx, root, &mut Cursor::new(root), 1).map(|_| ())
        });
    }

    #[test]
    fn header_tables_reject_impossible_ufrx_counts() {
        // The synthetic schema-11 header has 4 declaration bytes, 46 version
        // bytes, 32 save bytes, an 18-byte comment, 32 origin bytes,
        // 36 identity bytes, a 30-byte file name and a two-byte state.
        let lod_table_offset = 4 + 46 + 32 + 18 + 32 + 36 + 30 + 2;
        let (_, invariant_offset) = fixture(11);
        for (field, offset, original, count) in [
            ("LOD table count", lod_table_offset, 0, 65_536),
            ("header pair count", lod_table_offset + 4, 0, 65_536),
            (
                "external-reference count",
                invariant_offset + 2 + 4 + 4 + 4,
                1,
                1_000_000,
            ),
            // After the table count: caption 24 + state 4 + path 48 + library
            // id 4 + library name 18 + library state 2 + display name 22.
            (
                "reference state-group count",
                invariant_offset + 2 + 4 + 4 + 4 + 4 + 24 + 4 + 48 + 4 + 18 + 2 + 22,
                0,
                65_536,
            ),
        ] {
            let (mut bytes, _) = fixture(11);
            assert_eq!(View::u32_le_at(&bytes, offset), Some(original));
            bytes[offset..offset + 4].copy_from_slice(&u32::to_le_bytes(count));
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            // Only the 23 section-version entries precede these tables.
            policy.limits.max_collection_items = 23;
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
            let error = parse_stream(&ctx, root, stream_id(), &DocumentKind::Assembly)
                .expect_err("count cannot fit");
            assert!(
                matches!(error, CodecError::Malformed(ref detail)
                if detail == &format!("UFRxDoc {field} exceeds remaining payload")),
                "{field}: {error:?}"
            );
        }
    }

    #[test]
    fn occurrence_padding_scan_uses_bounded_constant_work() {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        push_occurrence(&mut bytes, 28, 2020, 9, 17, "");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The original one-unit cap admits the occurrence itself, then refuses
        // at the first empty property-list end probe.
        policy.limits.max_work_units = 1;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        assert!(matches!(
            parse_occurrences(&ctx, root, &mut Cursor::new(root), 28, 2020),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "admit UFRxDoc occurrence properties"
                    && limit.used == 1
                    && limit.additional == 1
        ));

        // One occurrence, two empty property lists, empty settings and the
        // outer end probe use 1 + 1 + 1 + 1 + 1 = 5 units. Padding remains a
        // bounded scan of at most nine u16 words with no collection storage.
        policy.limits.max_work_units = 5;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let occurrences = parse_occurrences(&ctx, root, &mut Cursor::new(root), 28, 2020)
            .expect("five units admit the occurrence and its empty lists");
        assert_eq!(occurrences[0].header_padding_words, 2);
        assert!(ctx.resource_refusal().is_none());
        let original = match ctx.charge_work(1, "probe visits") {
            Err(CodecError::ResourceLimit(limit)) => limit,
            other => panic!("one extra work unit must refuse: {other:?}"),
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.used, 5);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
        );
    }

    #[test]
    fn ufrx_unknown_property_tags_use_fixed_diagnostics() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Both tags render four hexadecimal bytes in a fixed diagnostic.
        // No variable text is scanned or retained by either refusal.
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_work_units = 0;
        let (limited, root) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root fits policy");
        assert!(matches!(
            parse_occurrence_value(&limited, &mut Cursor::new(root), 0xff),
            Err(CodecError::NotImplemented(detail))
                if detail == "UFRxDoc occurrence property tag 0xff is not implemented"
        ));
        assert!(matches!(
            parse_occurrence_item_value(&mut Cursor::new(root), 0xff),
            Err(CodecError::NotImplemented(detail))
                if detail == "UFRxDoc occurrence export item tag 0xff is not implemented"
        ));
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
    fn malformed_ufrx_detail_refuses_retained_limit_before_copy() {
        let bytes = crate::test_support::test_fixtures::fixture_with_ufrx(&[0; 2]);
        let arena = DecodeArena::new();
        let (setup, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("compound input fits service policy");
        let snapshot = CompoundSnapshot::new(&setup, root).expect("synthetic compound parses");
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (limited, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        assert!(matches!(
            super::parse(&limited, &snapshot, &DocumentKind::Part),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor malformed UFRx detail"
        ));
        assert!(matches!(
            super::parse(&setup, &snapshot, &DocumentKind::Part)
                .expect("malformed UFRx remains an inventory state"),
            UfrxState::Malformed { .. }
        ));
    }

    #[test]
    fn utf16_string_uses_exact_utf8_budget_before_allocation() {
        let bytes = [b'A', 0];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (limited, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        assert!(matches!(
            Cursor::new(root).utf16_counted(&limited, "value", 1),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain UFRxDoc string"
        ));
        policy.limits.max_retained_bytes = 1;
        let (admitted, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("exact context");
        assert_eq!(
            Cursor::new(root)
                .utf16_counted(&admitted, "value", 1)
                .expect("exact UTF-8 bytes admitted"),
            "A"
        );
    }

    #[test]
    fn parsed_document_box_uses_no_collection_slot() {
        let (bytes, _) = fixture(11);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The 23 section versions, one reference and one occurrence use 25
        // slots. The fixed document box adds no collection slot.
        policy.limits.max_collection_items = 25;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("fixture context");
        let document = parse_stream(&ctx, root, stream_id(), &DocumentKind::Assembly)
            .expect("fixture document parses within its collection budget");
        assert!(matches!(
            UfrxState::Parsed(Box::new(document)),
            UfrxState::Parsed(_)
        ));
        assert!(matches!(
            ctx.charge_collection_items(1, "probe collection slots"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.used == 25 && limit.additional == 1
        ));
    }

    fn stream_id() -> CompoundStreamId {
        let bytes = crate::test_support::test_fixtures::fixture(true);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic compound file fits policy");
        let snapshot = CompoundSnapshot::new(&ctx, root).expect("synthetic compound file parses");
        snapshot
            .stream(&ctx, "RSeStorage/RSeSegInfo")
            .expect("lookup admission")
            .expect("validated stream entry")
            .id()
    }

    #[test]
    fn schema_15_document_kind_uses_fixed_diagnostic() {
        let (bytes, _) = fixture(15);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The unsupported kind stores neither versions nor the file name.
        // Validation visits seven comment characters and thirteen file-name
        // characters, with one end probe for each string: 8 + 14 = 22. The
        // original cap now refuses the empty LOD-table end probe at unit 23.
        policy.limits.max_collection_items = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_work_units = 22;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        assert!(matches!(
            parse_stream(&ctx, root, stream_id(), &DocumentKind::Unknown),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "admit UFRxDoc LOD table entries"
                    && limit.used == 22
                    && limit.additional == 1
        ));

        // The two empty table end probes add two units to the string work.
        policy.limits.max_work_units = 24;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        assert!(
            matches!(parse_stream(&ctx, root, stream_id(), &DocumentKind::Unknown),
            Err(CodecError::NotImplemented(detail))
                if detail == "UFRxDoc schema 15 unknown header is not implemented")
        );
        assert!(ctx.resource_refusal().is_none());
        let original = match ctx.charge_retained(1, "probe unsupported header retention") {
            Err(CodecError::ResourceLimit(limit)) => limit,
            other => panic!("one extra retained byte must refuse: {other:?}"),
        };
        assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(original.used, 0);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
        );
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("service context");
        assert!(matches!(
            parse_stream(&ctx, root, stream_id(), &DocumentKind::Unknown),
            Err(CodecError::NotImplemented(_))
        ));
    }

    #[test]
    fn schema_15_unsupported_header_validates_file_name_without_storage() {
        let (mut bytes, _) = fixture(15);
        // Declaration, 27 version words, save fields, counted comment,
        // origin fields, database id/state and internal document id.
        let name_count_offset = 4 + 27 * 2 + 32 + (4 + 7 * 2) + 32 + (16 + 4 + 16);
        assert_eq!(View::u32_le_at(&bytes, name_count_offset), Some(13));
        let first_unit = name_count_offset + 4;
        bytes[first_unit..first_unit + 2].copy_from_slice(&0xd800_u16.to_le_bytes());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        assert!(
            matches!(parse_stream(&ctx, root, stream_id(), &DocumentKind::Unknown),
            Err(CodecError::Malformed(detail)) if detail == "UFRxDoc original file name is not UTF-16")
        );
    }

    #[test]
    fn unframed_lod_uses_fixed_diagnostic() {
        let (mut bytes, invariant_offset) = fixture(11);
        let lod_count_offset = invariant_offset + 2 + 4 + 4;
        bytes[lod_count_offset..lod_count_offset + 4].copy_from_slice(&1_u32.to_le_bytes());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // The 23 version words retain 46 bytes; the ASCII file name retains
        // 13 bytes. The bounded diagnostic adds none: 46 + 13 = 59.
        policy.limits.max_retained_bytes = 59;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        assert!(
            matches!(parse_stream(&ctx, root, stream_id(), &DocumentKind::Assembly),
            Err(CodecError::NotImplemented(detail)) if detail == "UFRxDoc contains 1 unframed LOD records")
        );
        assert!(
            matches!(ctx.charge_retained(1, "probe retained grammar storage"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.used == 59 && limit.additional == 1)
        );
        policy.limits.max_retained_bytes = 58;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("short context");
        assert!(
            matches!(parse_stream(&ctx, root, stream_id(), &DocumentKind::Assembly),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain UFRxDoc string"
                    && limit.used == 46 && limit.additional == 13)
        );
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("service context");
        assert!(matches!(
            parse_stream(&ctx, root, stream_id(), &DocumentKind::Assembly),
            Err(CodecError::NotImplemented(_))
        ));
    }

    #[test]
    fn retained_ufrx_grammar_uses_no_materialized_storage() {
        let (bytes, _) = fixture(11);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // These strings and vectors belong to the returned document. They
        // need retained admission, with no temporary materialization budget.
        policy.limits.max_materialized_bytes = 0;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("context");
        let document = parse_stream(&ctx, root, stream_id(), &DocumentKind::Assembly)
            .expect("retained document needs no materialized bytes");
        assert_eq!(document.schema, 11);
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
            assert_eq!(document.occurrences[0].file_reference_id, 7);
            assert_eq!(document.occurrences[0].occurrence_id, 42);
            assert_eq!(document.occurrences[0].title.as_deref(), Some("placed"));
            if schema == 15 {
                let representation = document
                    .representation
                    .as_ref()
                    .expect("schema 15 representation state");
                assert_eq!(representation.active_model_state, "Master");
                assert_eq!(
                    representation
                        .active_representation
                        .as_ref()
                        .map(|(name, _)| name.as_str()),
                    Some("Default")
                );
                assert_eq!(
                    representation
                        .active_representation
                        .as_ref()
                        .map(|(_, kind)| kind.as_str()),
                    Some("DesignView")
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
    fn schema_15_part_omits_assembly_representation_strings() {
        let (bytes, _) = fixture_for_kind(15, false);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic part UFRxDoc fits policy");

        let document = parse_stream(&ctx, root, stream_id(), &DocumentKind::Part)
            .expect("schema-15 part UFRxDoc parses");

        let representation = document.representation.expect("model-state header parses");
        assert_eq!(representation.active_representation, None);
        assert_eq!(representation.active_model_state, "Master");
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
        push_occurrence(&mut bytes, 28, 2020, 9, 17, "extended");
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic occurrence fits policy");
        let mut cursor = Cursor::new(root);

        let occurrences = parse_occurrences(&ctx, root, &mut cursor, 28, 2020)
            .expect("extended occurrence parses");

        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].file_reference_id, 9);
        assert_eq!(occurrences[0].occurrence_id, 17);
        assert_eq!(occurrences[0].title.as_deref(), Some("extended"));
        assert_eq!(occurrences[0].header_padding_words, 2);
        assert_eq!(cursor.position(), bytes.len());
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
        bytes.extend_from_slice(&[0; 4]);
        if schema == 15 {
            push_u16(&mut bytes, 0);
            if assembly {
                push_utf16(&mut bytes, "Default");
                push_utf16(&mut bytes, "DesignView");
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
        push_occurrence(&mut bytes, 18, 0, 7, 42, "placed");
        bytes.extend_from_slice(b"tail");
        (bytes, invariant_offset)
    }

    fn push_occurrence(
        bytes: &mut Vec<u8>,
        section_version: u16,
        save_year: u16,
        file_reference_id: u32,
        occurrence_id: u32,
        title: &str,
    ) {
        push_u32(bytes, 0);
        push_u32(bytes, file_reference_id);
        push_u32(bytes, occurrence_id);
        push_u32(bytes, 0);
        push_u32(bytes, 1);
        push_utf16(bytes, title);
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
        if save_year >= 2015 {
            bytes.push(0);
        }
        push_u32(bytes, 0x00ff_ffff);
        push_u32(bytes, 0);
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
            Ok((ctx, root)) => displayed_truncation(parse_schema_table(&ctx, root)),
            Err(error) => error.to_string(),
        };
        assert_eq!(
            text,
            "truncated input during section-version count at space 0 offset 2"
        );
    }
}
