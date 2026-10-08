// SPDX-License-Identifier: Apache-2.0
//! Bounded OLE property-set streams.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_container::compound::{CompoundEntry, CompoundSnapshot, CompoundStreamId};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

const BYTE_ORDER_LE: u16 = 0xfffe;
const MAX_STREAM_SIZE: usize = 2_097_152;
const MAX_PROPERTIES: usize = 65_536;
const VT_VECTOR: u16 = 0x1000;
const VT_VARIANT: u16 = 0x000c;

#[derive(Debug)]
pub(crate) struct PropertySetStream<'a> {
    pub(crate) version: u16,
    pub(crate) system_identifier: u32,
    pub(crate) clsid: [u8; 16],
    pub(crate) sections: Vec<PropertySection<'a>>,
}

#[derive(Debug)]
pub(crate) struct PropertySetDescriptor<'a> {
    pub(crate) stream: CompoundStreamId,
    pub(crate) path: String,
    pub(crate) state: PropertySetState<'a>,
}

#[derive(Debug)]
pub(crate) enum PropertySetState<'a> {
    Parsed(PropertySetStream<'a>),
    Malformed(String),
}

#[derive(Debug)]
pub(crate) struct PropertySection<'a> {
    pub(crate) fmtid: [u8; 16],
    pub(crate) code_page: Option<u16>,
    pub(crate) offsets_ordered: bool,
    pub(crate) dictionary_entries: usize,
    pub(crate) properties: Vec<Property<'a>>,
}

#[derive(Debug)]
pub(crate) struct Property<'a> {
    pub(crate) id: u32,
    pub(crate) name: Option<String>,
    pub(crate) value: PropertyValue<'a>,
    pub(crate) raw: View<'a>,
}

#[derive(Debug)]
pub(crate) enum PropertyValue<'a> {
    Empty {
        type_code: u16,
    },
    Signed {
        type_code: u16,
        value: i64,
    },
    Unsigned {
        type_code: u16,
        value: u64,
    },
    Float {
        type_code: u16,
        value: f64,
    },
    Bool {
        type_code: u16,
        value: bool,
    },
    Filetime {
        type_code: u16,
        value: u64,
    },
    String {
        type_code: u16,
        value: String,
    },
    Guid {
        type_code: u16,
        value: [u8; 16],
    },
    Binary {
        type_code: u16,
        value: View<'a>,
    },
    Clipboard {
        type_code: u16,
        format: u32,
        data: View<'a>,
    },
    Vector {
        type_code: u16,
        values: Vec<PropertyValue<'a>>,
    },
    Dictionary,
    Unknown {
        type_code: u16,
    },
}

impl PropertyValue<'_> {
    pub(crate) fn scalar_text(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<String>, CodecError> {
        let text = match self {
            Self::Signed { value, .. } => {
                ctx.format_retained(format_args!("{value}"), "retain OLE scalar text")?
            }
            Self::Unsigned { value, .. } => {
                ctx.format_retained(format_args!("{value}"), "retain OLE scalar text")?
            }
            Self::Float { value, .. } if value.is_finite() => {
                ctx.format_retained(format_args!("{value}"), "retain OLE scalar text")?
            }
            Self::Bool { value, .. } => {
                ctx.format_retained(format_args!("{value}"), "retain OLE scalar text")?
            }
            Self::Filetime { value, .. } => {
                ctx.format_retained(format_args!("{value}"), "retain OLE scalar text")?
            }
            Self::String { value, .. } => {
                ctx.copy_retained_text(value, "retain OLE scalar text")?
            }
            Self::Guid { value, .. } =>
                crate::pmdc::fixed_hex(ctx, value, "retain OLE scalar text")?,
            Self::Empty { .. }
            | Self::Float { .. }
            | Self::Binary { .. }
            | Self::Clipboard { .. }
            | Self::Vector { .. }
            | Self::Dictionary
            | Self::Unknown { .. } => return Ok(None),
        };
        Ok(Some(text))
    }
}

fn has_property_set_header(bytes: &[u8]) -> bool {
    View::u16_le_at(bytes, 0) == Some(BYTE_ORDER_LE)
        && matches!(View::u16_le_at(bytes, 2), Some(0 | 1))
        && matches!(View::u32_le_at(bytes, 24), Some(1 | 2))
}

pub(crate) fn inventory<'a>(
    ctx: &DecodeContext<'a>,
    snapshot: &CompoundSnapshot<'a>,
) -> Result<Vec<PropertySetDescriptor<'a>>, CodecError> {
    let mut property_sets = Vec::new();
    let mut entries = snapshot.entries().iter();
    while let Some(entry) = ctx.next_charged(&mut entries, "scan Inventor property-set streams")? {
        let CompoundEntry::Stream(stream) = entry else {
            continue;
        };
        if stream.logical_size() < 28
            || stream.logical_size() > cadmpeg_core::decode::u64_from_index(MAX_STREAM_SIZE)
        {
            continue;
        }
        // Streams under the RSe storage carry RSe records, never property
        // sets; opening them here would copy every fragmented segment stream
        // only to read its first bytes.
        if in_rse_storage(ctx, stream.path())? {
            continue;
        }
        let view = snapshot.open(ctx, stream)?;
        if !has_property_set_header(view.window())
            && View::u16_le_at(view.window(), 0) != Some(BYTE_ORDER_LE)
        {
            continue;
        }
        let state = match parse_property_set_stream(ctx, view) {
            Ok(property_set) => PropertySetState::Parsed(property_set),
            Err(error) => PropertySetState::Malformed(crate::issue_detail(
                ctx,
                error,
                "retain Inventor malformed property-set detail",
            )?),
        };
        ctx.push_vec(
            &mut property_sets,
            PropertySetDescriptor {
                stream: stream.id(),
                path: ctx.copy_retained_text(stream.path(), "retain Inventor property-set path")?,
                state,
            },
            "admit Inventor property-set streams",
        )?;
    }
    Ok(property_sets)
}

fn in_rse_storage(ctx: &DecodeContext<'_>, path: &str) -> Result<bool, CodecError> {
    match ctx.split_once(path, "/", "split Inventor property-set candidate path")? {
        Some((storage, _)) => ctx.eq_ignore_ascii_case(
            storage,
            "RSeStorage",
            "classify Inventor property-set candidate storage",
        ),
        None => Ok(false),
    }
}

pub(crate) fn parse_property_set_stream<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
) -> Result<PropertySetStream<'a>, CodecError> {
    let bytes = source.window();
    if bytes.len() > MAX_STREAM_SIZE {
        return Err(CodecError::malformed(format_args!(
            "OLE property-set stream exceeds {MAX_STREAM_SIZE} bytes"
        )));
    }
    let mut cursor = Cursor::new(source, "OLE property-set stream");
    if cursor.u16("byte order")? != BYTE_ORDER_LE {
        return Err(CodecError::Malformed(
            "OLE property-set byte order is not little-endian".into(),
        ));
    }
    let version = cursor.u16("version")?;
    if !matches!(version, 0 | 1) {
        return Err(CodecError::malformed(format_args!(
            "OLE property-set version {version} is invalid"
        )));
    }
    let system_identifier = cursor.u32("system identifier")?;
    let clsid = cursor.array("CLSID")?;
    let section_count = cursor.count("section count", 2)?;
    if section_count == 0 {
        return Err(CodecError::Malformed(
            "OLE property-set stream has no sections".into(),
        ));
    }
    let mut directories = Vec::new();
    let mut directories_storage = ctx.reserve_scoped(0, "admit OLE section directories")?;
    let mut fmtids = BTreeSet::new();
    let mut fmtids_storage = ctx.reserve_scoped(0, "admit OLE section FMTIDs")?;
    let mut entries = 0..section_count;
    while ctx.next_charged(&mut entries, "admit OLE section directories")?.is_some() {
        let fmtid = cursor.array("section FMTID")?;
        if !fmtids_storage
            .with_storage(|| ctx.insert_btree_set(&mut fmtids, fmtid, "admit OLE section FMTIDs"))?
        {
            return Err(CodecError::Malformed(
                "OLE property-set stream duplicates a section FMTID".into(),
            ));
        }
        let offset = cursor.offset("section offset")?;
        directories_storage.with_storage(|| {
            ctx.push_vec(
                &mut directories,
                (fmtid, offset),
                "admit OLE section directories",
            )
        })?;
    }
    let header_end = cursor.position();
    ctx.stable_sort_by(
        &mut directories,
        |value| &value.1,
        Ord::cmp,
        "OLE section directories sort",
    )?;
    let mut previous_end = header_end;
    let mut sections = ctx.vector_storage(section_count, "admit OLE property-set sections")?;
    let mut entries = directories.iter();
    while let Some(&(fmtid, offset)) = ctx.next_charged(&mut entries, "scan OLE section directories")? {
        if offset < previous_end || offset % 4 != 0 {
            return Err(CodecError::Malformed(
                "OLE property-set section ranges overlap or are not aligned".into(),
            ));
        }
        require_zero_range(ctx, bytes, previous_end, offset, "section gap")?;
        let mut section = crate::reader::at(source, source.start() + offset, "section size")?;
        let size =
            usize::try_from(crate::reader::u32(&mut section, "section size")?).map_err(|_| {
                CodecError::Malformed("Inventor numeric value exceeds target range".into())
            })?;
        let end = offset.checked_add(size).ok_or_else(|| {
            CodecError::Malformed("OLE property-set section range overflows".into())
        })?;
        if size < 8 || end > bytes.len() {
            return Err(CodecError::Malformed(
                "OLE property-set section range is invalid".into(),
            ));
        }
        let section_source = source
            .child(source.start() + offset, source.start() + end)
            .ok_or_else(|| {
                CodecError::Malformed("OLE property-set section view is invalid".into())
            })?;
        ctx.push_vec(
            &mut sections,
            parse_section(ctx, section_source, fmtid)?,
            "admit OLE property-set sections",
        )?;
        previous_end = end;
    }
    drop(directories);
    drop(directories_storage);
    drop(fmtids);
    drop(fmtids_storage);
    require_zero_range(ctx, bytes, previous_end, bytes.len(), "stream suffix")?;
    Ok(PropertySetStream {
        version,
        system_identifier,
        clsid,
        sections,
    })
}

fn parse_section<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    fmtid: [u8; 16],
) -> Result<PropertySection<'a>, CodecError> {
    let bytes = source.window();
    let mut cursor = Cursor::new(source, "OLE property-set section");
    let size = cursor.offset("size")?;
    if size != bytes.len() {
        return Err(CodecError::Malformed(
            "OLE property-set section size does not match its range".into(),
        ));
    }
    let property_count = cursor.count("property count", MAX_PROPERTIES)?;
    let directory_end = 8_usize
        .checked_add(property_count.checked_mul(8).ok_or_else(|| {
            CodecError::Malformed("OLE property directory length overflows".into())
        })?)
        .ok_or_else(|| CodecError::Malformed("OLE property directory range overflows".into()))?;
    // The directory is read entry by entry below, so a directory the section
    // cannot hold stops at the entry that runs out of bytes.
    let mut ids = BTreeSet::new();
    let mut ids_storage = ctx.reserve_scoped(0, "admit OLE property IDs")?;
    let mut directory = Vec::new();
    let mut directory_storage = ctx.reserve_scoped(0, "admit OLE property directory")?;
    let mut entries = 0..property_count;
    while ctx.next_charged(&mut entries, "admit OLE property directory")?.is_some() {
        let id = cursor.u32("property id")?;
        if !ids_storage
            .with_storage(|| ctx.insert_btree_set(&mut ids, id, "admit OLE property IDs"))?
        {
            return Err(CodecError::malformed(format_args!(
                "OLE property set duplicates property id {id}"
            )));
        }
        let offset = cursor.offset("property offset")?;
        if offset < directory_end || offset % 4 != 0 {
            return Err(CodecError::malformed(format_args!(
                "OLE property {id} has an invalid offset"
            )));
        }
        directory_storage.with_storage(|| {
            ctx.push_vec(&mut directory, (offset, id), "admit OLE property directory")
        })?;
    }
    let mut previous_offset = None;
    let offsets_ordered = ctx.all_by(
        &directory,
        |(offset, _)| {
            let ordered = previous_offset.is_none_or(|previous| previous < *offset);
            previous_offset = Some(*offset);
            Ok(ordered)
        },
        "check OLE property directory order",
    )?;
    ctx.sort_unstable_by(
        &mut directory,
        |value| value,
        Ord::cmp,
        "OLE property directory sort",
    )?;
    let mut previous_offset = None;
    let mut entries = directory.iter();
    while let Some((offset, _)) = ctx.next_charged(&mut entries, "check OLE property offsets")? {
        if previous_offset == Some(*offset) {
            return Err(CodecError::Malformed(
                "OLE properties have duplicate offsets".into(),
            ));
        }
        previous_offset = Some(*offset);
    }
    drop(ids);
    drop(ids_storage);
    if let Some((offset, _)) = directory.first() {
        require_zero_range(ctx, bytes, directory_end, *offset, "property-directory gap")?;
    }
    let (ranges, ranges_storage) = ctx.with_scoped_storage("admit OLE property ranges", || {
        ctx.try_collect_vec(
            directory.iter().enumerate().map(|(index, (start, id))| {
                let end = directory
                    .get(index + 1)
                    .map_or(bytes.len(), |(offset, _)| *offset);
                if end > bytes.len() || *start >= end {
                    return Err(CodecError::malformed(format_args!(
                        "OLE property {id} range is invalid"
                    )));
                }
                Ok((*id, *start, end))
            }),
            "admit OLE property ranges",
        )
    })?;
    drop(directory);
    drop(directory_storage);
    let code_page = match ctx.find_by(
        &ranges,
        |range| Ok(range.0 == 1),
        "find OLE code-page property",
    )? {
        Some((_, start, end)) => Some(parse_code_page(
            ctx,
            child(source, *start, *end, "code-page property")?,
        )?),
        None => None,
    };
    let mut names_storage = ctx.reserve_scoped(0, "admit OLE property dictionary entries")?;
    let names = match ctx.find_by(
        &ranges,
        |range| Ok(range.0 == 0),
        "find OLE property dictionary",
    )? {
        Some((_, start, end)) => parse_dictionary(
            ctx,
            child(source, *start, *end, "property dictionary")?,
            code_page,
            &mut names_storage,
        )?,
        None => BTreeMap::new(),
    };
    let mut properties = ctx.vector_storage(property_count, "admit OLE properties")?;
    let mut entries = ranges.iter();
    while let Some(&(id, start, end)) = ctx.next_charged(&mut entries, "parse OLE properties")? {
        let raw = source
            .child(source.start() + start, source.start() + end)
            .ok_or_else(|| CodecError::Malformed("OLE property view is invalid".into()))?;
        let value = if id == 0 {
            PropertyValue::Dictionary
        } else {
            parse_typed_value(ctx, raw, code_page)?
        };
        let name = if let Some(name) = ctx.get_btree_map(
            &names,
            &id,
            "find OLE property dictionary name",
        )? {
            Some(ctx.copy_retained_text(name, "retain OLE property name")?)
        } else {
            None
        };
        ctx.push_vec(
            &mut properties,
            Property {
                id,
                name,
                value,
                raw,
            },
            "admit OLE properties",
        )?;
    }
    let dictionary_entries = names.len();
    drop((names, names_storage));
    drop((ranges, ranges_storage));
    ctx.stable_sort_by(
        &mut properties,
        |value| &value.id,
        Ord::cmp,
        "OLE properties sort",
    )?;
    Ok(PropertySection {
        fmtid,
        code_page,
        offsets_ordered,
        dictionary_entries,
        properties,
    })
}

fn parse_code_page(ctx: &DecodeContext<'_>, source: View<'_>) -> Result<u16, CodecError> {
    let mut cursor = Cursor::new(source, "OLE code-page property");
    if cursor.u16("type")? != 2 || cursor.u16("type padding")? != 0 {
        return Err(CodecError::Malformed(
            "OLE code-page property is not a padded VT_I2".into(),
        ));
    }
    let code_page = cursor.u16("value")?;
    if cursor.u16("value padding")? != 0 {
        return Err(CodecError::Malformed(
            "OLE code-page property padding is nonzero".into(),
        ));
    }
    cursor.zero_finish(ctx)?;
    Ok(code_page)
}

fn parse_dictionary(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    code_page: Option<u16>,
    names_storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<BTreeMap<u32, String>, CodecError> {
    let mut cursor = Cursor::new(source, "OLE property dictionary");
    let count = cursor.count("entry count", MAX_PROPERTIES)?;
    // An entry is at least its id and size words.
    if cursor
        .view
        .counted(cadmpeg_core::decode::u64_from_index(count), 8)
        .is_none()
    {
        return Err(CodecError::Malformed(
            "OLE property dictionary count exceeds its range".into(),
        ));
    }
    let mut names = BTreeMap::new();
    let mut folded_names_storage = ctx.reserve_scoped(0, "admit OLE folded dictionary names")?;
    let mut folded_names = BTreeSet::new();
    let mut entries = 0..count;
    while ctx.next_charged(&mut entries, "admit OLE property dictionary entries")?.is_some() {
        let id = cursor.u32("entry id")?;
        let size = cursor.count("entry string size", MAX_STREAM_SIZE)?;
        let name = names_storage
            .with_storage(|| cursor.code_page_string(ctx, size, code_page, "entry name"))?;
        if id == 0 {
            return Err(CodecError::Malformed(
                "OLE property dictionary duplicates or names a reserved id".into(),
            ));
        }
        // Uppercase mapping has no context rule, so it maps each character
        // to at most three without the whole-text bound.
        let uppercase_name = folded_names_storage.with_storage(|| {
            ctx.collect_text(
                ctx.admit_iter(name.as_str(), "retain OLE dictionary uppercase name")?
                    .flat_map(char::to_uppercase),
                "retain OLE dictionary uppercase name",
            )
        })?;
        if names_storage
            .with_storage(|| {
                ctx.insert_btree_map(
                    &mut names,
                    id,
                    name,
                    "admit OLE property dictionary entries",
                )
            })?
            .is_some()
        {
            return Err(CodecError::Malformed(
                "OLE property dictionary duplicates or names a reserved id".into(),
            ));
        }
        if !folded_names_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut folded_names,
                uppercase_name,
                "admit OLE folded dictionary names",
            )
        })? {
            return Err(CodecError::Malformed(
                "OLE property dictionary duplicates a name".into(),
            ));
        }
        cursor.align4(ctx, "entry padding")?;
    }
    drop((folded_names, folded_names_storage));
    cursor.zero_finish(ctx)?;
    Ok(names)
}

fn parse_typed_value<'a>(
    ctx: &DecodeContext<'_>,
    raw: View<'a>,
    code_page: Option<u16>,
) -> Result<PropertyValue<'a>, CodecError> {
    let mut cursor = Cursor::new(raw, "OLE typed property");
    let type_code = cursor.u16("type")?;
    if cursor.u16("type padding")? != 0 {
        return Err(CodecError::Malformed(
            "OLE typed-property padding is nonzero".into(),
        ));
    }
    let value = if type_code & VT_VECTOR != 0 {
        parse_vector(ctx, raw, &mut cursor, type_code & !VT_VECTOR, code_page)?
    } else {
        parse_scalar(ctx, raw, &mut cursor, type_code, code_page, true)?
    };
    cursor.zero_finish(ctx)?;
    Ok(value)
}

fn parse_vector<'a>(
    ctx: &DecodeContext<'_>,
    raw: View<'a>,
    cursor: &mut Cursor<'a>,
    element_type: u16,
    code_page: Option<u16>,
) -> Result<PropertyValue<'a>, CodecError> {
    let count = cursor.count("vector element count", MAX_PROPERTIES)?;
    let mut values = ctx.vector_storage(count, "admit OLE property vector elements")?;
    let mut entries = 0..count;
    while ctx.next_charged(&mut entries, "admit OLE property vector elements")?.is_some() {
        if element_type == VT_VARIANT {
            let nested_type = cursor.u16("variant type")?;
            if cursor.u16("variant type padding")? != 0 {
                return Err(CodecError::Malformed(
                    "OLE vector variant padding is nonzero".into(),
                ));
            }
            ctx.push_vec(
                &mut values,
                parse_scalar(ctx, raw, cursor, nested_type, code_page, true)?,
                "admit OLE property vector elements",
            )?;
        } else {
            ctx.push_vec(
                &mut values,
                parse_scalar(ctx, raw, cursor, element_type, code_page, false)?,
                "admit OLE property vector elements",
            )?;
        }
    }
    cursor.align4(ctx, "vector padding")?;
    Ok(PropertyValue::Vector {
        type_code: element_type | VT_VECTOR,
        values,
    })
}

enum ScalarType {
    Empty,
    I2,
    I4,
    R4,
    R8,
    I8,
    Bool,
    I1,
    Ui1,
    Ui2,
    Ui4,
    Ui8,
    CodePageString,
    UnicodeString,
    Filetime,
    Binary,
    Clipboard,
    Guid,
    Unknown,
}

impl ScalarType {
    fn from_code(type_code: u16) -> Self {
        match type_code {
            0x0000 | 0x0001 => Self::Empty,
            0x0002 => Self::I2,
            0x0003 | 0x0016 | 0x000a => Self::I4,
            0x0004 => Self::R4,
            0x0005 | 0x0007 => Self::R8,
            0x0006 | 0x0014 => Self::I8,
            0x000b => Self::Bool,
            0x0010 => Self::I1,
            0x0011 => Self::Ui1,
            0x0012 => Self::Ui2,
            0x0013 | 0x0017 => Self::Ui4,
            0x0015 => Self::Ui8,
            0x001e | 0x0008 => Self::CodePageString,
            0x001f => Self::UnicodeString,
            0x0040 => Self::Filetime,
            0x0041 | 0x0046 => Self::Binary,
            0x0047 => Self::Clipboard,
            0x0048 => Self::Guid,
            _ => Self::Unknown,
        }
    }

    fn kind_name(&self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::I2 | Self::I4 | Self::I8 | Self::I1 => "signed",
            Self::R4 | Self::R8 => "float",
            Self::Bool => "bool",
            Self::Ui1 | Self::Ui2 | Self::Ui4 | Self::Ui8 => "unsigned",
            Self::CodePageString | Self::UnicodeString => "string",
            Self::Filetime => "filetime",
            Self::Binary => "binary",
            Self::Clipboard => "clipboard",
            Self::Guid => "guid",
            Self::Unknown => "unknown",
        }
    }
}

/// The property kind selected by the OLE type code.
pub(crate) fn property_kind_name(type_code: u16) -> &'static str {
    if type_code & VT_VECTOR != 0 {
        "vector"
    } else {
        ScalarType::from_code(type_code).kind_name()
    }
}

fn parse_scalar<'a>(
    ctx: &DecodeContext<'_>,
    raw: View<'a>,
    cursor: &mut Cursor<'a>,
    type_code: u16,
    code_page: Option<u16>,
    padded: bool,
) -> Result<PropertyValue<'a>, CodecError> {
    let value = match ScalarType::from_code(type_code) {
        ScalarType::Empty => PropertyValue::Empty { type_code },
        ScalarType::I2 => PropertyValue::Signed {
            type_code,
            value: i64::from(cursor.i16("VT_I2")?),
        },
        ScalarType::I4 => PropertyValue::Signed {
            type_code,
            value: i64::from(cursor.i32("VT_I4")?),
        },
        ScalarType::R4 => PropertyValue::Float {
            type_code,
            value: f64::from(f32::from_bits(cursor.u32("VT_R4")?)),
        },
        ScalarType::R8 => PropertyValue::Float {
            type_code,
            value: f64::from_bits(cursor.u64("VT_R8")?),
        },
        ScalarType::I8 => PropertyValue::Signed {
            type_code,
            value: cursor.i64("VT_I8")?,
        },
        ScalarType::Bool => {
            let value = cursor.i16("VT_BOOL")?;
            if !matches!(value, 0 | -1) {
                return Err(CodecError::Malformed(
                    "OLE VT_BOOL is neither false nor true".into(),
                ));
            }
            PropertyValue::Bool {
                type_code,
                value: value != 0,
            }
        }
        ScalarType::I1 => PropertyValue::Signed {
            type_code,
            value: i64::from(cursor.u8("VT_I1")?.cast_signed()),
        },
        ScalarType::Ui1 => PropertyValue::Unsigned {
            type_code,
            value: u64::from(cursor.u8("VT_UI1")?),
        },
        ScalarType::Ui2 => PropertyValue::Unsigned {
            type_code,
            value: u64::from(cursor.u16("VT_UI2")?),
        },
        ScalarType::Ui4 => PropertyValue::Unsigned {
            type_code,
            value: u64::from(cursor.u32("VT_UI4")?),
        },
        ScalarType::Ui8 => PropertyValue::Unsigned {
            type_code,
            value: cursor.u64("VT_UI8")?,
        },
        ScalarType::CodePageString => {
            let size = cursor.count("code-page string size", MAX_STREAM_SIZE)?;
            let value = cursor.code_page_string(ctx, size, code_page, "string")?;
            cursor.align4(ctx, "string padding")?;
            PropertyValue::String { type_code, value }
        }
        ScalarType::UnicodeString => {
            let count = cursor.count("Unicode string length", MAX_STREAM_SIZE / 2)?;
            let value = cursor.unicode_string(ctx, count, "Unicode string")?;
            cursor.align4(ctx, "Unicode string padding")?;
            PropertyValue::String { type_code, value }
        }
        ScalarType::Filetime => PropertyValue::Filetime {
            type_code,
            value: cursor.u64("FILETIME")?,
        },
        ScalarType::Binary => {
            let size = cursor.count("BLOB size", MAX_STREAM_SIZE)?;
            let start = cursor.position();
            cursor.take(size, "BLOB")?;
            let value = PropertyValue::Binary {
                type_code,
                value: child(raw, start, cursor.position(), "BLOB")?,
            };
            cursor.align4(ctx, "BLOB padding")?;
            value
        }
        ScalarType::Clipboard => {
            let size = cursor.count("clipboard size", MAX_STREAM_SIZE)?;
            if size < 4 {
                return Err(CodecError::Malformed(
                    "OLE clipboard property is shorter than its format field".into(),
                ));
            }
            let format = cursor.u32("clipboard format")?;
            let start = cursor.position();
            cursor.take(size - 4, "clipboard data")?;
            let value = PropertyValue::Clipboard {
                type_code,
                format,
                data: child(raw, start, cursor.position(), "clipboard data")?,
            };
            cursor.align4(ctx, "clipboard padding")?;
            value
        }
        ScalarType::Guid => PropertyValue::Guid {
            type_code,
            value: cursor.array("CLSID")?,
        },
        ScalarType::Unknown => {
            cursor.skip_to_end();
            PropertyValue::Unknown { type_code }
        }
    };
    if padded && !matches!(type_code, 0x0000 | 0x0001) {
        cursor.align4(ctx, "scalar padding")?;
    }
    Ok(value)
}

fn child<'a>(raw: View<'a>, start: usize, end: usize, field: &str) -> Result<View<'a>, CodecError> {
    raw.child(raw.start() + start, raw.start() + end)
        .ok_or_else(|| CodecError::malformed(format_args!("OLE {field} view is invalid")))
}

fn require_zero_range(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
    field: &str,
) -> Result<(), CodecError> {
    let gap = bytes.get(start..end).ok_or_else(|| {
        CodecError::malformed(format_args!("OLE property-set {field} is invalid"))
    })?;
    if ctx.any_by(
        gap,
        |byte| Ok(*byte != 0),
        "validate OLE property-set zero range",
    )? {
        return Err(CodecError::malformed(format_args!(
            "OLE property-set {field} is nonzero"
        )));
    }
    Ok(())
}

fn decode_code_page(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    code_page: Option<u16>,
) -> Result<String, CodecError> {
    if code_page == Some(1200) {
        if !bytes.len().is_multiple_of(2) {
            return Err(CodecError::Malformed(
                "OLE Unicode code-page string has an odd byte length".into(),
            ));
        }
        let value =
            ctx.utf16le_text(bytes, bytes.len() / 2, false, "retain OLE property string")?;
        return require_and_remove_null(ctx, value, "OLE Unicode code-page string");
    }
    let (content, had_null) = if bytes.last() == Some(&0) {
        (&bytes[..bytes.len() - 1], true)
    } else {
        (bytes, false)
    };
    if !bytes.is_empty() && !had_null {
        return Err(CodecError::Malformed(
            "OLE code-page string has no null terminator".into(),
        ));
    }
    let page = code_page.unwrap_or(1252);
    let Some(encoding) = encoding_for_code_page(page) else {
        return Err(CodecError::NotImplemented(ctx.format_retained(
            format_args!("OLE code page {page} is not implemented"),
            "retain OLE unsupported code-page detail",
        )?));
    };
    let (selected_encoding, source) = encoding_rs::Encoding::for_bom(content)
        .map_or((encoding, content), |(selected, bom_len)| {
            (selected, &content[bom_len..])
        });
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(source.len()),
        "decode OLE code-page string",
    )?;
    // One decoding pass writes into retained capacity that grows with need:
    // each step makes room for the rest of the input as one byte per input
    // byte (at least one character), and the decoder stops when that room is
    // full. A step consumes at least a third of the input it had room for, so
    // the steps are logarithmic and the capacity stays within the output plus
    // the input still unread.
    let mut decoder = selected_encoding.new_decoder_without_bom_handling();
    let mut decoded = String::new();
    let mut read = 0_usize;
    loop {
        ctx.charge_work(1, "step OLE code-page decoder")?;
        let input = source.get(read..).ok_or_else(|| {
            CodecError::Malformed("OLE code-page decoder offset is invalid".into())
        })?;
        ctx.try_reserve_retained_text(
            &mut decoded,
            input.len().max(4),
            "retain OLE property string",
        )?;
        let written_before = decoded.len();
        let (result, consumed) =
            decoder.decode_to_string_without_replacement(input, &mut decoded, true);
        read = read.checked_add(consumed).ok_or_else(|| {
            ctx.refuse_codec_limit("decode OLE code-page string", u64::MAX, u64::MAX)
        })?;
        match result {
            encoding_rs::DecoderResult::InputEmpty if read == source.len() => break,
            encoding_rs::DecoderResult::InputEmpty => {
                return Err(CodecError::malformed(
                    "OLE code-page decoder ended before its input",
                ));
            }
            encoding_rs::DecoderResult::OutputFull
                if consumed != 0 || decoded.len() != written_before => {}
            encoding_rs::DecoderResult::OutputFull => {
                return Err(CodecError::malformed(
                    "OLE code-page decoder made no progress",
                ));
            }
            encoding_rs::DecoderResult::Malformed(..) => {
                return Err(CodecError::malformed(format_args!(
                    "OLE code-page {page} string is malformed"
                )));
            }
        }
    }
    Ok(decoded)
}

fn encoding_for_code_page(code_page: u16) -> Option<&'static encoding_rs::Encoding> {
    match code_page {
        65001 => Some(encoding_rs::UTF_8),
        874 => Some(encoding_rs::WINDOWS_874),
        932 => Some(encoding_rs::SHIFT_JIS),
        936 => Some(encoding_rs::GBK),
        949 => Some(encoding_rs::EUC_KR),
        950 => Some(encoding_rs::BIG5),
        1250 => Some(encoding_rs::WINDOWS_1250),
        1251 => Some(encoding_rs::WINDOWS_1251),
        1252 => Some(encoding_rs::WINDOWS_1252),
        1253 => Some(encoding_rs::WINDOWS_1253),
        1254 => Some(encoding_rs::WINDOWS_1254),
        1255 => Some(encoding_rs::WINDOWS_1255),
        1256 => Some(encoding_rs::WINDOWS_1256),
        1257 => Some(encoding_rs::WINDOWS_1257),
        1258 => Some(encoding_rs::WINDOWS_1258),
        _ => None,
    }
}

fn require_and_remove_null(
    ctx: &DecodeContext<'_>,
    mut value: String,
    field: &str,
) -> Result<String, CodecError> {
    if value.is_empty() {
        return Ok(value);
    }
    if ctx
        .strip_suffix(&value, "\0", "validate OLE string terminator")?
        .is_none()
    {
        return Err(CodecError::malformed(format_args!(
            "{field} has no null terminator"
        )));
    }
    if value.pop() != Some('\0') {
        return Err(CodecError::malformed(format_args!(
            "{field} has no null terminator"
        )));
    }
    Ok(value)
}

struct Cursor<'a> {
    view: View<'a>,
    scope: &'static str,
}

impl<'a> Cursor<'a> {
    const fn new(view: View<'a>, scope: &'static str) -> Self {
        Self { view, scope }
    }

    fn position(&self) -> usize {
        self.view.read_len()
    }

    fn skip_to_end(&mut self) {
        self.view.seek_to_end();
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

    fn i16(&mut self, field: &'static str) -> Result<i16, CodecError> {
        crate::reader::i16(&mut self.view, field)
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

    fn i64(&mut self, field: &'static str) -> Result<i64, CodecError> {
        crate::reader::i64(&mut self.view, field)
    }

    fn array<const N: usize>(&mut self, field: &'static str) -> Result<[u8; N], CodecError> {
        crate::reader::array(&mut self.view, field)
    }

    fn count(&mut self, field: &'static str, maximum: usize) -> Result<usize, CodecError> {
        let value = self.offset(field)?;
        if value > maximum {
            return Err(CodecError::malformed(format_args!(
                "{} {field} exceeds {maximum}",
                self.scope
            )));
        }
        Ok(value)
    }

    fn offset(&mut self, field: &'static str) -> Result<usize, CodecError> {
        usize::try_from(self.u32(field)?)
            .map_err(|_| CodecError::malformed(format_args!("{} {field} is too large", self.scope)))
    }

    fn align4(&mut self, ctx: &DecodeContext<'_>, field: &'static str) -> Result<(), CodecError> {
        let padding = (4 - self.position() % 4) % 4;
        let padding = self.take(padding, field)?;
        if ctx.any_by(
            padding,
            |byte| Ok(*byte != 0),
            "validate OLE alignment padding",
        )? {
            return Err(CodecError::malformed(format_args!(
                "{} {field} is nonzero",
                self.scope
            )));
        }
        Ok(())
    }

    fn code_page_string(
        &mut self,
        ctx: &DecodeContext<'_>,
        size: usize,
        code_page: Option<u16>,
        field: &'static str,
    ) -> Result<String, CodecError> {
        let byte_len = if code_page == Some(1200) {
            size.checked_mul(2).ok_or_else(|| {
                CodecError::malformed(format_args!("{} {field} length overflows", self.scope))
            })?
        } else {
            size
        };
        decode_code_page(ctx, self.take(byte_len, field)?, code_page)
    }

    fn unicode_string(
        &mut self,
        ctx: &DecodeContext<'_>,
        count: usize,
        field: &'static str,
    ) -> Result<String, CodecError> {
        let value = crate::reader::utf16_text(
            ctx,
            &mut self.view,
            count,
            field,
            "retain OLE Unicode property string",
        )?;
        require_and_remove_null(ctx, value, field)
    }

    fn zero_finish(self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        let zero_suffix = match self.view.window().get(self.position()..) {
            Some(rest) => ctx.all_by(rest, |byte| Ok(*byte == 0), "validate OLE trailing bytes")?,
            None => false,
        };
        if zero_suffix {
            Ok(())
        } else {
            Err(CodecError::malformed(format_args!(
                "{} has nonzero trailing bytes",
                self.scope
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_container::compound::CompoundSnapshot;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};

    use crate::test_support::truncation::located_truncation;

    use super::{
        has_property_set_header, parse_dictionary, parse_property_set_stream, Cursor,
        PropertySetStream, PropertyValue, BYTE_ORDER_LE,
    };
    use cadmpeg_core::decode::{DecodeContext, View};
    use cadmpeg_core::CodecError;

    #[test]
    fn truncated_property_vector_does_not_precharge_unread_elements() {
        for count in [1_u32, 512] {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&(super::VT_VECTOR | 3).to_le_bytes());
            bytes.extend_from_slice(&0_u16.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            // The first range step executes, then the first VT_I4 read fails.
            // No other vector step or terminal probe executes.
            policy.limits.max_work_units = 1;
            let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("truncated vector context");
            assert!(matches!(super::parse_typed_value(&ctx, view, None),
                Err(CodecError::Truncated { .. })));
            ctx.finish_session().expect("unread elements consume no work");
        }
    }

    #[test]
    fn property_dictionary_name_lookup_refuses_before_search() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&68_u32.to_le_bytes());
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        for (id, offset) in [(1_u32, 32_u32), (0, 40), (2, 60)] {
            bytes.extend_from_slice(&id.to_le_bytes());
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&1200_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        for unit in [u16::from(b'a'), u16::from(b'b'), u16::from(b'c'), 0] {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&3_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&42_i32.to_le_bytes());
        let arena = DecodeArena::new();
        let (service, view) = DecodeContext::from_root_bytes(
            &bytes, &arena, &DecodePolicy::service(),
        ).expect("property section context");
        let section = super::parse_section(&service, view, [0; 16])
            .expect("property dictionary section");
        assert_eq!(section.dictionary_entries, 1);
        assert_eq!(section.properties[2].id, 2);
        assert_eq!(section.properties[2].name.as_deref(), Some("abc"));

        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("lookup refusal context");
        let probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits, "find OLE property dictionary name", None,
        );
        let Err(CodecError::ResourceLimit(limit)) = super::parse_section(&ctx, view, [0; 16]) else {
            panic!("dictionary lookup must use the owning tree operation");
        };
        drop(probe);
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "find OLE property dictionary name");
        assert!(limit.additional > 0);
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }

    #[test]
    fn guid_scalar_text_admits_storage_without_fixed_work() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 32;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("GUID context");
        let value = PropertyValue::Guid { type_code: 0x48, value: [0xaf; 16] };
        assert_eq!(value.scalar_text(&ctx).expect("fixed GUID text"),
            Some("afafafafafafafafafafafafafafafaf".to_owned()));
    }

    #[test]
    fn malformed_property_set_detail_refuses_retained_limit_before_copy() {
        let mut bytes = crate::test_support::test_fixtures::fixture_with_ufrx(&[0xfe, 0xff]);
        let entry_start = 512 + 3 * 128;
        let name = "ZProperty";
        bytes[entry_start..entry_start + 64].fill(0);
        for (index, unit) in name.encode_utf16().enumerate() {
            let offset = entry_start + index * 2;
            bytes[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
        }
        let name_len =
            u16::try_from((name.encode_utf16().count() + 1) * 2).expect("fixture value fits u16");
        bytes[entry_start + 64..entry_start + 66].copy_from_slice(&name_len.to_le_bytes());
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
            super::inventory(&limited, &snapshot),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor malformed property-set detail"
        ));
        let inventory = super::inventory(&setup, &snapshot)
            .expect("malformed property set remains an inventory entry");
        assert!(matches!(
            inventory.first().map(|entry| &entry.state),
            Some(super::PropertySetState::Malformed(_))
        ));
    }

    #[test]
    fn dictionary_temporary_name_copies_refuse_scoped_limit_before_allocation() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        for unit in "abc\0".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let arena = DecodeArena::new();
        let (service, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("service context");
        let mut names_storage = service
            .reserve_scoped(0, "admit OLE property dictionary entries")
            .expect("dictionary name storage reservation");
        assert_eq!(
            parse_dictionary(
                &service,
                root,
                Some(1200),
                &mut names_storage,
            )
            .expect("dictionary admitted")
            .get(&2)
            .map(String::as_str),
            Some("abc")
        );
        // The decoded name holds 4 bytes until its terminator is removed. Its
        // uppercase form grows one byte per character (7), the name then moves
        // into its 452-byte map node (459), and the uppercase form into its
        // 408-byte set node (867). The name is not copied.
        for (cap, operation) in [
            (6, "retain OLE dictionary uppercase name"),
            (458, "admit OLE property dictionary entries"),
            (866, "admit OLE folded dictionary names"),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (limited, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
            let mut names_storage = limited
                .reserve_scoped(0, "admit OLE property dictionary entries")
                .expect("dictionary name storage reservation");
            assert!(matches!(
                parse_dictionary(
                    &limited,
                    root,
                    Some(1200),
                    &mut names_storage,
                ),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::MaterializedBytes
                        && limit.operation == operation
            ));
        }
    }

    #[test]
    fn dictionary_scratch_releases_before_large_property_sort() {
        fn amortized_capacity<T>(count: usize) -> usize {
            let minimum = match std::mem::size_of::<T>() {
                1 => 8,
                2..=1024 => 4,
                _ => 1,
            };
            let mut capacity = 0_usize;
            for required in 1..=count {
                if required > capacity {
                    capacity = capacity
                        .checked_mul(2)
                        .expect("small fixture capacity fits")
                        .max(required)
                        .max(minimum);
                }
            }
            capacity
        }

        fn tree_node_bytes<K, V>() -> usize {
            let alignment = std::mem::align_of::<K>()
                .max(std::mem::align_of::<V>())
                .max(std::mem::align_of::<usize>());
            (std::mem::size_of::<K>() + std::mem::size_of::<V>()) * 11
                + 16 * std::mem::size_of::<usize>()
                + 2 * alignment
        }

        fn tree_nodes(count: usize) -> usize {
            if count == 0 {
                0
            } else {
                (count - 1) / 5 + 1
            }
        }

        // Twenty-one properties use the two-array branch of stable_sort_by.
        let property_count = 21_usize;
        let directory_entry_size = std::mem::size_of::<(usize, u32)>();
        let range_size = std::mem::size_of::<(u32, usize, usize)>();
        let directory_capacity = amortized_capacity::<(usize, u32)>(property_count);
        let range_capacity = amortized_capacity::<(u32, usize, usize)>(property_count);
        let directory_bytes = directory_capacity * directory_entry_size;
        let range_bytes = range_capacity * range_size;

        let mut id_directory_growth_peak = 0_usize;
        let mut prior_directory_capacity = 0_usize;
        for inserted in 1..=property_count {
            let ids_bytes = tree_nodes(inserted) * tree_node_bytes::<u32, ()>();
            let current_directory_bytes = prior_directory_capacity * directory_entry_size;
            id_directory_growth_peak = id_directory_growth_peak
                .max(ids_bytes + current_directory_bytes);
            if inserted > prior_directory_capacity {
                let next_directory_capacity = prior_directory_capacity
                    .checked_mul(2)
                    .expect("small fixture capacity fits")
                    .max(inserted)
                    .max(4);
                let next_directory_bytes = next_directory_capacity * directory_entry_size;
                id_directory_growth_peak = id_directory_growth_peak
                    .max(ids_bytes + current_directory_bytes + next_directory_bytes);
                prior_directory_capacity = next_directory_capacity;
            }
        }

        // At the 17th range, the 16-tuple allocation overlaps its 32-tuple
        // replacement while the 32-entry directory remains live.
        let range_growth_peak = directory_bytes
            + (range_capacity / 2) * range_size
            + range_bytes;
        let dictionary_peak = range_bytes
            + ("abc".len() + 1)
            + "abc".to_uppercase().len()
            + tree_node_bytes::<u32, String>()
            + tree_node_bytes::<String, ()>();
        let sort_scratch = 2 * property_count * std::mem::size_of::<usize>();
        let property_sort_peak = sort_scratch;
        // The limit must admit each earlier stage too. Directory sorting is
        // unstable and uses no scratch; directory growth still overlaps its
        // ID tree, and range growth overlaps the admitted directory vector.
        let materialized_peak = range_growth_peak
            .max(dictionary_peak)
            .max(id_directory_growth_peak)
            .max(property_sort_peak);

        let directory_end = 8 + property_count * 8;
        let mut values = Vec::<(u32, Vec<u8>)>::with_capacity(property_count);
        let mut dictionary = Vec::new();
        dictionary.extend_from_slice(&1_u32.to_le_bytes());
        dictionary.extend_from_slice(&2_u32.to_le_bytes());
        dictionary.extend_from_slice(&4_u32.to_le_bytes());
        for unit in "abc\0".encode_utf16() {
            dictionary.extend_from_slice(&unit.to_le_bytes());
        }
        values.push((0, dictionary));

        let mut code_page = Vec::new();
        code_page.extend_from_slice(&2_u16.to_le_bytes());
        code_page.extend_from_slice(&0_u16.to_le_bytes());
        code_page.extend_from_slice(&1200_u16.to_le_bytes());
        code_page.extend_from_slice(&0_u16.to_le_bytes());
        values.push((1, code_page));
        for value in 2..=20_u32 {
            let mut property = Vec::new();
            property.extend_from_slice(&3_u16.to_le_bytes());
            property.extend_from_slice(&0_u16.to_le_bytes());
            property.extend_from_slice(
                &i32::try_from(value)
                    .expect("small fixture value fits")
                    .to_le_bytes(),
            );
            values.push((value, property));
        }

        let mut directory = Vec::with_capacity(property_count);
        let mut property_bytes = Vec::new();
        let mut offset = directory_end;
        for (id, value) in values {
            directory.push((id, offset));
            property_bytes.extend_from_slice(&value);
            offset += value.len();
        }
        let mut bytes = Vec::new();
        bytes.extend_from_slice(
            &u32::try_from(offset)
                .expect("small fixture size fits")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(
            &u32::try_from(property_count)
                .expect("small fixture count fits")
                .to_le_bytes(),
        );
        for (id, property_offset) in directory {
            bytes.extend_from_slice(&id.to_le_bytes());
            bytes.extend_from_slice(
                &u32::try_from(property_offset)
                    .expect("small offset fits")
                    .to_le_bytes(),
            );
        }
        bytes.extend_from_slice(&property_bytes);
        assert_eq!(bytes.len(), offset);

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes =
            u64::try_from(materialized_peak).expect("small fixture peak fits");
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("source-derived property-set limit");
        let section = super::parse_section(&ctx, root, [0; 16])
            .expect("dictionary scratch releases before large property sort");
        assert_eq!(section.dictionary_entries, 1);
        assert_eq!(section.properties.len(), property_count);
        assert_eq!(section.properties[2].id, 2);
        assert_eq!(section.properties[2].name.as_deref(), Some("abc"));
        assert!(matches!(
            &section.properties[2].value,
            PropertyValue::Signed { value: 2, .. }
        ));
        ctx.finish_session()
            .expect("all scratch reservations release after parsing");
    }

    #[test]
    fn code_page_text_wider_than_its_input_decodes_in_one_growing_pass() {
        // Each windows-1252 0xE9 byte is two UTF-8 bytes, so the first room,
        // one byte per input byte, fills halfway and the decoder grows into it.
        let mut bytes = vec![0xe9_u8; 1000];
        bytes.push(0);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("service context");
        let decoded = super::decode_code_page(&ctx, &bytes, Some(1252)).expect("decoded text");
        assert_eq!(decoded, "é".repeat(1000));
    }

    #[test]
    fn unsupported_code_page_detail_requires_exact_retained_budget() {
        let bytes = [0];
        let arena = DecodeArena::new();
        let message = "OLE code page 65000 is not implemented";
        let operation = "retain OLE unsupported code-page detail";
        let required = u64::try_from(message.len()).expect("diagnostic length fits u64");
        for cap in [0, required - 1, required] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
            let error = Cursor::new(root, "OLE code-page string")
                .code_page_string(&ctx, 1, Some(65000), "value")
                .expect_err("unsupported code page");
            if cap < required {
                assert!(matches!(
                    error,
                    CodecError::ResourceLimit(limit)
                        if limit.dimension == ResourceDimension::RetainedBytes
                            && limit.operation == operation
                ));
            } else {
                assert!(matches!(
                    error,
                    CodecError::NotImplemented(detail) if detail == message
                ));
            }
        }
    }

    #[test]
    fn code_page_string_refuses_exact_utf8_copy_before_decode() {
        let bytes = [0x80, 0];
        let arena = DecodeArena::new();
        let (service, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("service context");
        assert_eq!(
            Cursor::new(root, "OLE code-page string")
                .code_page_string(&service, 2, Some(1252), "value")
                .expect("code-page string admitted"),
            "€"
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 2;
        let (limited, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        assert!(matches!(
            Cursor::new(root, "OLE code-page string")
                .code_page_string(&limited, 2, Some(1252), "value"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain OLE property string"
        ));
    }

    #[test]
    fn unicode_property_string_uses_exact_utf8_budget_before_decode() {
        let bytes = [b'A', 0, 0, 0];
        let arena = DecodeArena::new();
        let (service, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("service context");
        assert_eq!(
            Cursor::new(root, "OLE Unicode property")
                .unicode_string(&service, 2, "value")
                .expect("Unicode string admitted"),
            "A"
        );
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 1;
        let (limited, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        assert!(matches!(
            Cursor::new(root, "OLE Unicode property").unicode_string(&limited, 2, "value"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain OLE Unicode property string"
        ));
        policy.limits.max_retained_bytes = 2;
        let (admitted, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("exact context");
        assert_eq!(
            Cursor::new(root, "OLE Unicode property")
                .unicode_string(&admitted, 2, "value")
                .expect("exact UTF-8 bytes admitted"),
            "A"
        );
    }

    #[test]
    fn section_collections_refuse_each_limit_before_materialization() {
        let bytes = fixture();
        let arena = DecodeArena::new();
        // Section insertion follows FMTID/directory slots and three IDs, directory entries, ranges, and properties.
        for (cap, operation) in [
            (0, "admit OLE section FMTIDs"),
            (1, "admit OLE section directories"),
            (2 + 4 * 3, "admit OLE property-set sections"),
        ] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (limited, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
            assert!(matches!(
                parse_property_set_stream(&limited, root),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::CollectionItems
                        && limit.operation == operation
            ));
        }
        let (service, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("service context");
        assert_eq!(
            parse_property_set_stream(&service, root)
                .expect("property set admitted")
                .sections
                .len(),
            1
        );
    }

    #[test]
    fn scalar_text_refuses_retained_limit_before_copy_or_format() {
        let text = PropertyValue::String {
            type_code: 30,
            value: "scalar".into(),
        };
        let number = PropertyValue::Signed {
            type_code: 3,
            value: -42,
        };
        let arena = DecodeArena::new();
        let bytes = b"fixture";
        let (service, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("service context");
        assert_eq!(
            text.scalar_text(&service).expect("text admitted"),
            Some("scalar".into())
        );
        assert_eq!(
            number.scalar_text(&service).expect("number admitted"),
            Some("-42".into())
        );
        for (value, cap) in [(&text, 5), (&number, 2)] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (limited, _) =
                DecodeContext::from_root_bytes(bytes, &arena, &policy).expect("limited context");
            assert!(matches!(
                value.scalar_text(&limited),
                Err(CodecError::ResourceLimit(limit))
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == "retain OLE scalar text"
            ));
        }
    }

    #[test]
    fn property_set_parses_unicode_metadata_and_preview_blob() {
        let bytes = fixture();
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic property set fits policy");
        let parsed = parse_property_set_stream(&ctx, root).expect("property set parses");
        assert_eq!(parsed.sections.len(), 1);
        let section = &parsed.sections[0];
        assert_eq!(section.code_page, Some(1200));
        assert!(matches!(
            &section.properties[1].value,
            PropertyValue::String { value, .. } if value == "Synthetic title"
        ));
        assert!(matches!(
            &section.properties[2].value,
            PropertyValue::Binary { value, .. } if value.window().starts_with(b"\x89PNG\r\n\x1a\n")
        ));
    }

    #[test]
    fn property_set_rejects_duplicate_ids_and_trailing_bytes() {
        let mut duplicate = fixture();
        duplicate[60..64].copy_from_slice(&1_u32.to_le_bytes());
        with_parse(&duplicate, |result| assert!(result.is_err()));

        let mut trailing = fixture();
        trailing.push(1);
        with_parse(&trailing, |result| assert!(result.is_err()));
    }

    #[test]
    fn header_probe_requires_version_and_section_count() {
        let bytes = fixture();
        assert!(has_property_set_header(&bytes));
        assert!(!has_property_set_header(&bytes[..20]));
        assert!(!has_property_set_header(b"not a property set"));
    }

    #[test]
    fn property_set_retains_noncanonical_directory_order() {
        let mut bytes = fixture();
        let first: [u8; 8] = bytes[56..64].try_into().expect("first directory entry");
        let second: [u8; 8] = bytes[64..72].try_into().expect("second directory entry");
        bytes[56..64].copy_from_slice(&second);
        bytes[64..72].copy_from_slice(&first);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default())
            .expect("synthetic property set fits policy");
        let parsed = parse_property_set_stream(&ctx, root).expect("unordered directory parses");
        assert!(!parsed.sections[0].offsets_ordered);
    }

    #[test]
    fn a_truncated_property_set_read_is_located_and_names_its_field() {
        let empty: &[u8] = &[];
        let scope = "OLE property-set stream";
        for (field, text) in [
            (
                "byte order",
                located_truncation(
                    Cursor::new(View::over_retained(empty), scope).u16("byte order"),
                ),
            ),
            (
                "property id",
                located_truncation(
                    Cursor::new(View::over_retained(empty), scope).u32("property id"),
                ),
            ),
            (
                "VT_I8",
                located_truncation(Cursor::new(View::over_retained(empty), scope).i64("VT_I8")),
            ),
            (
                "CLSID",
                located_truncation(
                    Cursor::new(View::over_retained(empty), scope).array::<16>("CLSID"),
                ),
            ),
            (
                "BLOB",
                located_truncation(Cursor::new(View::over_retained(empty), scope).take(4, "BLOB")),
            ),
        ] {
            assert_eq!(text, format!("Truncated {field} at offset 0"));
        }
    }

    #[test]
    fn a_truncated_unicode_property_string_is_located_and_names_its_field() {
        let arena = DecodeArena::new();
        let bytes = [0x41, 0x00];
        let text = match DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()) {
            Ok((ctx, root)) => located_truncation(
                Cursor::new(root, "OLE typed property").unicode_string(&ctx, 4, "Unicode string"),
            ),
            Err(error) => error.to_string(),
        };
        assert_eq!(text, "Truncated Unicode string at offset 0");
    }

    /// A one-section stream header naming `offset` as the section's start.
    fn one_section_header(offset: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&BYTE_ORDER_LE.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0x0002_0006_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&FMTID_SUMMARY);
        bytes.extend_from_slice(&offset.to_le_bytes());
        bytes
    }

    #[test]
    fn a_truncated_section_size_is_located_and_names_its_field() {
        let mut bytes = one_section_header(48);
        bytes.extend_from_slice(&[0, 0]);
        with_parse(&bytes, |parsed| {
            assert_eq!(
                located_truncation(parsed),
                "Truncated section size at offset 48"
            );
        });
    }

    #[test]
    fn a_property_directory_the_section_cannot_hold_is_located_and_names_its_field() {
        let mut bytes = one_section_header(48);
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&100_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&808_u32.to_le_bytes());
        with_parse(&bytes, |parsed| {
            assert_eq!(
                located_truncation(parsed),
                "Truncated property id at offset 64"
            );
        });
    }

    fn with_parse(bytes: &[u8], test: impl FnOnce(Result<PropertySetStream<'_>, CodecError>)) {
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::default())
            .expect("synthetic property set fits policy");
        test(parse_property_set_stream(&ctx, root));
    }

    fn fixture() -> Vec<u8> {
        let title = typed_lpwstr("Synthetic title");
        let preview = typed_blob(b"\x89PNG\r\n\x1a\nsynthetic");
        let directory_len = 8 + 3 * 8;
        let code_page_offset = directory_len;
        let title_offset = code_page_offset + 8;
        let preview_offset = title_offset + title.len();
        let section_size = preview_offset + preview.len();

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&BYTE_ORDER_LE.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&0x0002_0006_u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&FMTID_SUMMARY);
        bytes.extend_from_slice(&48_u32.to_le_bytes());
        bytes.extend_from_slice(
            &(u32::try_from(section_size).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        for (id, offset) in [
            (1_u32, code_page_offset),
            (2, title_offset),
            (17, preview_offset),
        ] {
            bytes.extend_from_slice(&id.to_le_bytes());
            bytes.extend_from_slice(
                &(u32::try_from(offset).expect("fixture value fits u32")).to_le_bytes(),
            );
        }
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&1200_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(&title);
        bytes.extend_from_slice(&preview);
        bytes
    }

    fn typed_lpwstr(value: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x001f_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        let units = value.encode_utf16().chain([0]).collect::<Vec<_>>();
        bytes.extend_from_slice(
            &(u32::try_from(units.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        bytes
    }

    fn typed_blob(value: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0x0041_u16.to_le_bytes());
        bytes.extend_from_slice(&0_u16.to_le_bytes());
        bytes.extend_from_slice(
            &(u32::try_from(value.len()).expect("fixture value fits u32")).to_le_bytes(),
        );
        bytes.extend_from_slice(value);
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        bytes
    }

    const FMTID_SUMMARY: [u8; 16] = [
        0xe0, 0x85, 0x9f, 0xf2, 0xf9, 0x4f, 0x68, 0x10, 0xab, 0x91, 0x08, 0x00, 0x2b, 0x27, 0xb3,
        0xd9,
    ];
}
