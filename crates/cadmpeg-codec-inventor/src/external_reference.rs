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
    pub(crate) active_representation: Option<String>,
    pub(crate) active_representation_kind: Option<String>,
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
    let Some(stream) = snapshot.stream("UFRxDoc") else {
        return Ok(UfrxState::Absent);
    };
    let source = snapshot.open(ctx, stream)?;
    Ok(match parse_stream(ctx, source, document_kind) {
        Ok(document) => UfrxState::Parsed(Box::new(UfrxDocument {
            stream: stream.id(),
            schema: document.schema,
            section_versions: document.section_versions,
            original_file_name: document.original_file_name,
            caption: document.caption,
            representation: document.representation,
            model_states: document.model_states,
            references: document.references,
            embedded_references: document.embedded_references,
            occurrences: document.occurrences,
            unparsed_tail: document.unparsed_tail,
        })),
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
            detail: crate::issue_detail(error)?,
        },
    })
}

struct ParsedUfrx<'a> {
    schema: u16,
    section_versions: Vec<u16>,
    original_file_name: String,
    caption: String,
    representation: Option<UfrxRepresentationState>,
    model_states: Vec<UfrxModelState<'a>>,
    references: Vec<InventorExternalReference>,
    embedded_references: Vec<InventorEmbeddedReference<'a>>,
    occurrences: Vec<UfrxOccurrence<'a>>,
    unparsed_tail: View<'a>,
}

fn parse_stream<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    document_kind: &DocumentKind,
) -> Result<ParsedUfrx<'a>, CodecError> {
    let mut cursor = Cursor::new(source);
    let schema = cursor.u16("schema")?;
    if !(11..=15).contains(&schema) {
        return Err(CodecError::NotImplemented(format!(
            "UFRxDoc schema {schema} is not implemented"
        )));
    }
    let section_count = cursor.count16("section-version count", 256)?;
    if section_count < 5 {
        return Err(CodecError::Malformed(
            "UFRxDoc section-version table is too short".into(),
        ));
    }
    ctx.charge_collection_items(
        section_count as u64,
        "admit UFRxDoc section-version entries",
    )?;
    let mut section_versions = Vec::with_capacity(section_count);
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
    cursor.u32("header padding")?;
    cursor.take(16, "internal document id")?;
    let original_file_name = cursor.utf16(ctx, "original file name", 65_536)?;
    cursor.u16("part flags")?;

    let lod_toc_count = cursor.count32("LOD table count", 65_536)?;
    ctx.charge_collection_items(lod_toc_count as u64, "admit UFRxDoc LOD table entries")?;
    for _ in 0..lod_toc_count {
        cursor.u16("LOD value")?;
        cursor.u16("LOD value")?;
        cursor.utf16(ctx, "LOD name", 65_536)?;
        cursor.take(2, "LOD state")?;
    }

    let pair_count = cursor.count32("header pair count", 65_536)?;
    ctx.charge_collection_items(pair_count as u64, "admit UFRxDoc header pairs")?;
    for _ in 0..pair_count {
        cursor.utf16(ctx, "header pair key", 65_536)?;
        cursor.utf16(ctx, "header pair value", 65_536)?;
    }
    cursor.take(4, "active LOD state")?;
    let assembly_representation = if schema == 15 {
        match document_kind {
            DocumentKind::Assembly => true,
            DocumentKind::Part => false,
            DocumentKind::Drawing | DocumentKind::Presentation | DocumentKind::Unknown(_) => {
                return Err(CodecError::NotImplemented(format!(
                    "UFRxDoc schema 15 {} header is not implemented",
                    document_kind.label()
                )));
            }
        }
    } else {
        false
    };
    let representation = if schema == 15 {
        let prefix = cursor.u16("schema 15 representation prefix")?;
        let (active_representation, active_representation_kind) = if assembly_representation {
            (
                Some(cursor.utf16(ctx, "active representation", 65_536)?),
                Some(cursor.utf16(ctx, "active representation kind", 65_536)?),
            )
        } else {
            (None, None)
        };
        Some(UfrxRepresentationState {
            prefix,
            active_representation,
            active_representation_kind,
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
            cursor.utf16(ctx, "active design view", 65_536)?;
        }
        if section_versions[2] >= 7 {
            cursor.take(4, "secondary active LOD state")?;
        }
        cursor.u16("header version flags")?;
        None
    };
    cursor.u32("highest occurrence id")?;
    cursor.u16("next LOD id")?;
    let invariant = cursor.u16("header invariant")?;
    if invariant != 1 {
        return Err(CodecError::malformed(format_args!(
            "UFRxDoc header invariant is {invariant}, expected 1"
        )));
    }
    cursor.u32("highest file-reference id")?;
    cursor.u32("highest embedded-reference id")?;
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
    cursor.u32("external-reference header state")?;
    ctx.charge_collection_items(reference_count as u64, "admit Inventor external references")?;
    let mut references = Vec::with_capacity(reference_count);
    for _ in 0..reference_count {
        let path = cursor.utf16(ctx, "external path", 65_536)?;
        let library_id = cursor.i32("library id")?;
        let library_name = cursor.utf16(ctx, "library name", 65_536)?;
        cursor.u16("reference state")?;
        let display_prefix = cursor.peek_u32("reference display-name prefix")?;
        if display_prefix & 0xffff_0000 != 0 && cursor.u16("reference display-name padding")? != 0 {
            return Err(CodecError::Malformed(
                "UFRxDoc reference display-name padding is nonzero".into(),
            ));
        }
        let display_name = cursor.utf16(ctx, "reference display name", 65_536)?;
        let state_count = cursor.count32("reference state-group count", 65_536)?;
        ctx.charge_collection_items(
            state_count as u64,
            "admit Inventor external-reference state groups",
        )?;
        let mut state_groups = Vec::with_capacity(state_count);
        for _ in 0..state_count {
            state_groups.push([
                cursor.u16("reference state group")?,
                cursor.u16("reference state group")?,
                cursor.u16("reference state group")?,
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
    Ok(ParsedUfrx {
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
    ctx.charge_collection_items(count as u64, "admit UFRxDoc embedded references")?;
    let mut references = Vec::with_capacity(count);
    for _ in 0..count {
        let start = cursor.position();
        let value_0 = cursor.u32("embedded-reference value")?;
        let filetime = cursor.u64("embedded-reference FILETIME")?;
        let value_1 = cursor.u32("embedded-reference value")?;
        let extended_value = if section_version >= 7 {
            Some(cursor.u32("embedded-reference extended value")?)
        } else {
            None
        };
        let value_2 = cursor.u32("embedded-reference value")?;
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
    ctx.charge_collection_items(count as u64, "admit UFRxDoc occurrences")?;
    let mut occurrences = Vec::with_capacity(count);
    for _ in 0..count {
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
        parse_occurrence_section(ctx, cursor)?;
        parse_occurrence_section(ctx, cursor)?;
        parse_occurrence_settings(ctx, cursor)?;
        parse_occurrence_export(ctx, cursor, save_year)?;
        let record = source
            .child(source.start() + start, source.start() + cursor.position())
            .ok_or_else(|| CodecError::Malformed("UFRxDoc occurrence range is invalid".into()))?;
        occurrences.push(UfrxOccurrence {
            end_string_flag,
            file_reference_id,
            occurrence_id,
            header_value,
            title,
            header_padding_words,
            source: record,
        });
    }
    if count == 0 {
        cursor.u32("empty occurrence padding")?;
    }
    Ok(occurrences)
}

fn parse_occurrence_section(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
) -> Result<(), CodecError> {
    cursor.u32("occurrence section value")?;
    let count = cursor.count32("occurrence section property count", 65_536)?;
    ctx.charge_collection_items(count as u64, "admit UFRxDoc occurrence properties")?;
    for _ in 0..count {
        cursor.boolean("occurrence property presence")?;
        let tag = cursor.u8("occurrence property tag")?;
        cursor.u32("occurrence property value")?;
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
    ctx.charge_collection_items(count as u64, "admit UFRxDoc occurrence settings")?;
    for _ in 0..count {
        cursor.utf16(ctx, "occurrence setting name", 65_536)?;
        cursor.take(16, "occurrence setting id")?;
        cursor.utf8(ctx, "occurrence setting value", 65_536)?;
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
            cursor.utf16(ctx, "occurrence export name", 65_536)?;
            cursor.take(16, "occurrence export id")?;
            cursor.utf8(ctx, "occurrence export value", 65_536)?;
        } else {
            let count = cursor.count32("occurrence export count", 65_536)?;
            ctx.charge_collection_items(count as u64, "admit UFRxDoc occurrence exports")?;
            for _ in 0..count {
                cursor.utf16(ctx, "occurrence export name", 65_536)?;
                parse_occurrence_items(ctx, cursor)?;
                cursor.take(12, "occurrence export trailer")?;
                if save_year >= 2018 {
                    cursor.u8("occurrence export padding")?;
                }
            }
        }
    } else {
        cursor.u32("occurrence export count")?;
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
    ctx.charge_collection_items(count as u64, "admit UFRxDoc occurrence export items")?;
    for _ in 0..count {
        cursor.boolean("occurrence export item presence")?;
        let tag = cursor.u8("occurrence export item tag")?;
        let value_count = cursor.count32("occurrence export item value count", 65_536)?;
        ctx.charge_collection_items(value_count as u64, "admit UFRxDoc occurrence export values")?;
        for _ in 0..value_count {
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
            cursor.utf16(ctx, "occurrence property string", 65_536)?;
        }
        0x07 | 0x0d | 0x0f | 0x10 | 0x1d => {
            cursor.u8("occurrence property byte")?;
        }
        0x19 => {
            cursor.u32("occurrence property integer")?;
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
            cursor.u32("occurrence export item integer")?;
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

fn require_u32(actual: u32, expected: u32, field: &str) -> Result<(), CodecError> {
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
    ctx.charge_collection_items(count as u64, "admit UFRxDoc model states")?;
    let mut states = Vec::with_capacity(count);
    for _ in 0..count {
        let prefix = cursor.u8("model-state prefix")?;
        let name = cursor.utf16(ctx, "model-state name", 65_536)?;
        let state = [
            cursor.u16("model-state state")?,
            cursor.u16("model-state state")?,
        ];
        let prefix_count = cursor.u32("model-state prefix count")?;
        let parameter_count = cursor.count32("model-state parameter count", 1_000_000)?;
        ctx.charge_collection_items(
            parameter_count as u64,
            "admit UFRxDoc model-state parameters",
        )?;
        let mut parameters = Vec::with_capacity(parameter_count);
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
        cursor.take(77, "model-state suffix")?;
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

fn parse_schema_table(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
) -> Result<(u16, Vec<u16>), CodecError> {
    let mut cursor = Cursor::new(source);
    let schema = cursor.u16("schema")?;
    let section_count = cursor.count16("section-version count", 256)?;
    ctx.charge_collection_items(
        section_count as u64,
        "admit UFRxDoc section-version entries",
    )?;
    let mut section_versions = Vec::with_capacity(section_count);
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
        self.view.position().saturating_sub(self.view.start())
    }

    fn take(&mut self, len: usize, field: &str) -> Result<&'a [u8], CodecError> {
        let _ = self.position().checked_add(len).ok_or_else(|| {
            CodecError::malformed(format_args!("UFRxDoc {field} range overflows"))
        })?;
        self.view
            .take(len)
            .ok_or_else(|| CodecError::malformed(format_args!("truncated UFRxDoc {field}")))
    }

    fn u8(&mut self, field: &str) -> Result<u8, CodecError> {
        Ok(self.take(1, field)?[0])
    }

    fn u16(&mut self, field: &str) -> Result<u16, CodecError> {
        Ok(View::u16_le_at(self.take(2, field)?, 0).expect("two-byte field"))
    }

    fn u32(&mut self, field: &str) -> Result<u32, CodecError> {
        Ok(View::u32_le_at(self.take(4, field)?, 0).expect("four-byte field"))
    }

    fn i32(&mut self, field: &str) -> Result<i32, CodecError> {
        Ok(View::i32_le_at(self.take(4, field)?, 0).expect("four-byte field"))
    }

    fn u64(&mut self, field: &str) -> Result<u64, CodecError> {
        Ok(View::u64_le_at(self.take(8, field)?, 0).expect("eight-byte field"))
    }

    fn array<const N: usize>(&mut self, field: &str) -> Result<[u8; N], CodecError> {
        Ok(self
            .take(N, field)?
            .try_into()
            .expect("cursor returned requested fixed length"))
    }

    fn peek_u32(&self, field: &str) -> Result<u32, CodecError> {
        self.peek_u32_at(0, field)
    }

    fn peek_u16(&self, field: &str) -> Result<u16, CodecError> {
        let mut view = self.view;
        view.u16_le()
            .ok_or_else(|| CodecError::malformed(format_args!("truncated UFRxDoc {field}")))
    }

    fn peek_u32_at(&self, relative: usize, field: &str) -> Result<u32, CodecError> {
        let _ = self.position().checked_add(relative).ok_or_else(|| {
            CodecError::malformed(format_args!("UFRxDoc {field} range overflows"))
        })?;
        let mut view = self.view;
        view.skip(relative)
            .and_then(|()| view.u32_le())
            .ok_or_else(|| CodecError::malformed(format_args!("truncated UFRxDoc {field}")))
    }

    fn count16(&mut self, field: &str, maximum: usize) -> Result<usize, CodecError> {
        let value = self.u16(field)? as usize;
        if value > maximum {
            return Err(CodecError::malformed(format_args!(
                "UFRxDoc {field} exceeds {maximum}"
            )));
        }
        Ok(value)
    }

    fn count32(&mut self, field: &str, maximum: usize) -> Result<usize, CodecError> {
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
        field: &str,
        maximum: usize,
    ) -> Result<String, CodecError> {
        let count = self.count32(field, maximum)?;
        self.utf16_counted(ctx, field, count)
    }

    fn utf16_counted(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &str,
        count: usize,
    ) -> Result<String, CodecError> {
        let len = count.checked_mul(2).ok_or_else(|| {
            CodecError::malformed(format_args!("UFRxDoc {field} length overflows"))
        })?;
        ctx.charge_retained(len as u64, "retain UFRxDoc string", None)?;
        let _ = self.position().checked_add(len).ok_or_else(|| {
            CodecError::malformed(format_args!("UFRxDoc {field} range overflows"))
        })?;
        self.view.utf16_le(count).ok_or_else(|| {
            if self.view.remaining() < len {
                CodecError::malformed(format_args!("truncated UFRxDoc {field}"))
            } else {
                CodecError::malformed(format_args!("UFRxDoc {field} is not UTF-16"))
            }
        })
    }

    fn utf8(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &str,
        maximum: usize,
    ) -> Result<String, CodecError> {
        let count = self.count32(field, maximum)?;
        ctx.charge_retained(count as u64, "retain UFRxDoc string", None)?;
        let value = self.take(count, field)?;
        std::str::from_utf8(value)
            .map(str::to_owned)
            .map_err(|_| CodecError::malformed(format_args!("UFRxDoc {field} is not UTF-8")))
    }

    fn boolean(&mut self, field: &str) -> Result<bool, CodecError> {
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
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    use super::*;

    #[test]
    fn supported_schemas_frame_external_references_and_retain_the_tail() {
        for schema in 11..=15 {
            let (bytes, _) = fixture(schema);
            let arena = DecodeArena::new();
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
                    .expect("synthetic UFRxDoc fits policy");
            let document = parse_stream(&ctx, root, &DocumentKind::Assembly)
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
                    representation.active_representation.as_deref(),
                    Some("Default")
                );
                assert_eq!(
                    representation.active_representation_kind.as_deref(),
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

        let document =
            parse_stream(&ctx, root, &DocumentKind::Part).expect("schema-15 part UFRxDoc parses");

        let representation = document.representation.expect("model-state header parses");
        assert_eq!(representation.active_representation, None);
        assert_eq!(representation.active_representation_kind, None);
        assert_eq!(representation.active_model_state, "Master");
        assert_eq!(document.model_states.len(), 1);
        assert_eq!(document.references.len(), 1);
    }

    #[test]
    fn unsupported_schema_is_rejected() {
        let (bytes, _) = fixture(16);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic UFRxDoc fits policy");
        assert!(matches!(
            parse_stream(&ctx, root, &DocumentKind::Assembly),
            Err(CodecError::NotImplemented(_))
        ));
    }

    #[test]
    fn schema_11_rejects_nonzero_header_invariant() {
        let (mut bytes, invariant_offset) = fixture(11);
        bytes[invariant_offset..invariant_offset + 2].copy_from_slice(&2_u16.to_le_bytes());
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic UFRxDoc fits policy");
        assert!(parse_stream(&ctx, root, &DocumentKind::Assembly).is_err());
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

    fn push_utf16(bytes: &mut Vec<u8>, value: &str) {
        let units = value.encode_utf16().collect::<Vec<_>>();
        push_u32(bytes, units.len() as u32);
        for unit in units {
            push_u16(bytes, unit);
        }
    }

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}
