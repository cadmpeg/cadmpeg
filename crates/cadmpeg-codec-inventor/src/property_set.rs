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
    pub(crate) names: BTreeMap<u32, String>,
    pub(crate) properties: Vec<Property<'a>>,
}

#[derive(Debug)]
pub(crate) struct Property<'a> {
    pub(crate) id: u32,
    pub(crate) name: Option<String>,
    pub(crate) type_code: Option<u16>,
    pub(crate) value: PropertyValue<'a>,
    pub(crate) raw: View<'a>,
}

#[derive(Debug)]
pub(crate) enum PropertyValue<'a> {
    Empty,
    Signed(i64),
    Unsigned(u64),
    Float(f64),
    Bool(bool),
    Filetime(u64),
    String(String),
    Guid([u8; 16]),
    Binary(View<'a>),
    Clipboard { format: u32, data: View<'a> },
    Vector(Vec<PropertyValue<'a>>),
    Dictionary,
    Unknown,
}

impl PropertyValue<'_> {
    pub(crate) fn scalar_text(&self) -> Option<String> {
        match self {
            Self::Signed(value) => Some(value.to_string()),
            Self::Unsigned(value) => Some(value.to_string()),
            Self::Float(value) if value.is_finite() => Some(value.to_string()),
            Self::Bool(value) => Some(value.to_string()),
            Self::Filetime(value) => Some(value.to_string()),
            Self::String(value) => Some(value.clone()),
            Self::Guid(value) => Some(hex(value)),
            Self::Empty
            | Self::Float(_)
            | Self::Binary(_)
            | Self::Clipboard { .. }
            | Self::Vector(_)
            | Self::Dictionary
            | Self::Unknown => None,
        }
    }
}

pub(crate) fn has_property_set_header(bytes: &[u8]) -> bool {
    View::u16_le_at(bytes, 0) == Some(BYTE_ORDER_LE)
        && matches!(View::u16_le_at(bytes, 2), Some(0 | 1))
        && matches!(View::u32_le_at(bytes, 24), Some(1 | 2))
}

pub(crate) fn inventory<'a>(
    ctx: &DecodeContext<'a>,
    snapshot: &CompoundSnapshot<'a>,
) -> Result<Vec<PropertySetDescriptor<'a>>, CodecError> {
    let mut property_sets = Vec::new();
    for entry in snapshot.entries() {
        let CompoundEntry::Stream(stream) = entry else {
            continue;
        };
        if stream.logical_size() < 28 || stream.logical_size() > MAX_STREAM_SIZE as u64 {
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
            Err(error) => PropertySetState::Malformed(crate::issue_detail(error)?),
        };
        property_sets.push(PropertySetDescriptor {
            stream: stream.id(),
            path: stream.path().into(),
            state,
        });
    }
    ctx.charge_collection_items(
        property_sets.len() as u64,
        "admit Inventor property-set streams",
    )?;
    Ok(property_sets)
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
    ctx.charge_collection_items(section_count as u64, "admit OLE property-set sections")?;
    let mut directories = Vec::with_capacity(section_count);
    let mut fmtids = BTreeSet::new();
    for _ in 0..section_count {
        let fmtid = cursor.array("section FMTID")?;
        if !fmtids.insert(fmtid) {
            return Err(CodecError::Malformed(
                "OLE property-set stream duplicates a section FMTID".into(),
            ));
        }
        directories.push((fmtid, cursor.offset("section offset")?));
    }
    let header_end = cursor.position();
    directories.sort_by_key(|(_, offset)| *offset);
    let mut previous_end = header_end;
    let mut sections = Vec::with_capacity(section_count);
    for (fmtid, offset) in directories {
        if offset < previous_end || offset % 4 != 0 {
            return Err(CodecError::Malformed(
                "OLE property-set section ranges overlap or are not aligned".into(),
            ));
        }
        require_zero_range(bytes, previous_end, offset, "section gap")?;
        let size = View::u32_le_at(bytes, offset).ok_or_else(|| {
            CodecError::Malformed("truncated OLE property-set section size".into())
        })? as usize;
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
        sections.push(parse_section(ctx, section_source, fmtid)?);
        previous_end = end;
    }
    require_zero_range(bytes, previous_end, bytes.len(), "stream suffix")?;
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
    ctx.charge_collection_items(property_count as u64, "admit OLE properties")?;
    let directory_end = 8_usize
        .checked_add(property_count.checked_mul(8).ok_or_else(|| {
            CodecError::Malformed("OLE property directory length overflows".into())
        })?)
        .ok_or_else(|| CodecError::Malformed("OLE property directory range overflows".into()))?;
    if directory_end > bytes.len() {
        return Err(CodecError::Malformed(
            "truncated OLE property directory".into(),
        ));
    }
    let mut ids = BTreeSet::new();
    let mut directory = Vec::with_capacity(property_count);
    for _ in 0..property_count {
        let id = cursor.u32("property id")?;
        if !ids.insert(id) {
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
        directory.push((offset, id));
    }
    let offsets_ordered = directory.windows(2).all(|pair| pair[0].0 < pair[1].0);
    directory.sort_unstable();
    for pair in directory.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(CodecError::Malformed(
                "OLE properties have duplicate offsets".into(),
            ));
        }
    }
    if let Some((offset, _)) = directory.first() {
        require_zero_range(bytes, directory_end, *offset, "property-directory gap")?;
    }
    let ranges = directory
        .iter()
        .enumerate()
        .map(|(index, (start, id))| {
            let end = directory
                .get(index + 1)
                .map_or(bytes.len(), |(offset, _)| *offset);
            if end > bytes.len() || *start >= end {
                return Err(CodecError::malformed(format_args!(
                    "OLE property {id} range is invalid"
                )));
            }
            Ok((*id, *start, end))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let code_page = ranges
        .iter()
        .find(|(id, _, _)| *id == 1)
        .map(|(_, start, end)| parse_code_page(child(source, *start, *end, "code-page property")?))
        .transpose()?;
    let names = ranges
        .iter()
        .find(|(id, _, _)| *id == 0)
        .map(|(_, start, end)| {
            parse_dictionary(
                ctx,
                child(source, *start, *end, "property dictionary")?,
                code_page,
            )
        })
        .transpose()?
        .unwrap_or_default();
    let mut properties = Vec::with_capacity(property_count);
    for (id, start, end) in ranges {
        let raw = source
            .child(source.start() + start, source.start() + end)
            .ok_or_else(|| CodecError::Malformed("OLE property view is invalid".into()))?;
        let (type_code, value) = if id == 0 {
            (None, PropertyValue::Dictionary)
        } else {
            let (type_code, value) = parse_typed_value(ctx, raw, code_page)?;
            (Some(type_code), value)
        };
        properties.push(Property {
            id,
            name: names.get(&id).cloned(),
            type_code,
            value,
            raw,
        });
    }
    properties.sort_by_key(|property| property.id);
    Ok(PropertySection {
        fmtid,
        code_page,
        offsets_ordered,
        names,
        properties,
    })
}

fn parse_code_page(source: View<'_>) -> Result<u16, CodecError> {
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
    cursor.zero_finish()?;
    Ok(code_page)
}

fn parse_dictionary(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    code_page: Option<u16>,
) -> Result<BTreeMap<u32, String>, CodecError> {
    let mut cursor = Cursor::new(source, "OLE property dictionary");
    let count = cursor.count("entry count", MAX_PROPERTIES)?;
    ctx.charge_collection_items(count as u64, "admit OLE property dictionary entries")?;
    let mut names = BTreeMap::new();
    let mut folded_names = BTreeSet::new();
    for _ in 0..count {
        let id = cursor.u32("entry id")?;
        let size = cursor.count("entry string size", MAX_STREAM_SIZE)?;
        let name = cursor.code_page_string(ctx, size, code_page, "entry name")?;
        if id == 0 || names.insert(id, name.clone()).is_some() {
            return Err(CodecError::Malformed(
                "OLE property dictionary duplicates or names a reserved id".into(),
            ));
        }
        if !folded_names.insert(name.to_uppercase()) {
            return Err(CodecError::Malformed(
                "OLE property dictionary duplicates a name".into(),
            ));
        }
        cursor.align4("entry padding")?;
    }
    cursor.zero_finish()?;
    Ok(names)
}

fn parse_typed_value<'a>(
    ctx: &DecodeContext<'_>,
    raw: View<'a>,
    code_page: Option<u16>,
) -> Result<(u16, PropertyValue<'a>), CodecError> {
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
    cursor.zero_finish()?;
    Ok((type_code, value))
}

fn parse_vector<'a>(
    ctx: &DecodeContext<'_>,
    raw: View<'a>,
    cursor: &mut Cursor<'a>,
    element_type: u16,
    code_page: Option<u16>,
) -> Result<PropertyValue<'a>, CodecError> {
    let count = cursor.count("vector element count", MAX_PROPERTIES)?;
    ctx.charge_collection_items(count as u64, "admit OLE property vector elements")?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        if element_type == VT_VARIANT {
            let nested_type = cursor.u16("variant type")?;
            if cursor.u16("variant type padding")? != 0 {
                return Err(CodecError::Malformed(
                    "OLE vector variant padding is nonzero".into(),
                ));
            }
            values.push(parse_scalar(
                ctx,
                raw,
                cursor,
                nested_type,
                code_page,
                true,
            )?);
        } else {
            values.push(parse_scalar(
                ctx,
                raw,
                cursor,
                element_type,
                code_page,
                false,
            )?);
        }
    }
    cursor.align4("vector padding")?;
    Ok(PropertyValue::Vector(values))
}

fn parse_scalar<'a>(
    ctx: &DecodeContext<'_>,
    raw: View<'a>,
    cursor: &mut Cursor<'a>,
    type_code: u16,
    code_page: Option<u16>,
    padded: bool,
) -> Result<PropertyValue<'a>, CodecError> {
    let value = match type_code {
        0x0000 | 0x0001 => PropertyValue::Empty,
        0x0002 => PropertyValue::Signed(cursor.i16("VT_I2")? as i64),
        0x0003 | 0x0016 | 0x000a => PropertyValue::Signed(cursor.i32("VT_I4")? as i64),
        0x0004 => PropertyValue::Float(f32::from_bits(cursor.u32("VT_R4")?) as f64),
        0x0005 | 0x0007 => PropertyValue::Float(f64::from_bits(cursor.u64("VT_R8")?)),
        0x0006 | 0x0014 => PropertyValue::Signed(cursor.i64("VT_I8")?),
        0x000b => {
            let value = cursor.i16("VT_BOOL")?;
            if !matches!(value, 0 | -1) {
                return Err(CodecError::Malformed(
                    "OLE VT_BOOL is neither false nor true".into(),
                ));
            }
            PropertyValue::Bool(value != 0)
        }
        0x0010 => PropertyValue::Signed(cursor.u8("VT_I1")? as i8 as i64),
        0x0011 => PropertyValue::Unsigned(cursor.u8("VT_UI1")? as u64),
        0x0012 => PropertyValue::Unsigned(cursor.u16("VT_UI2")? as u64),
        0x0013 | 0x0017 => PropertyValue::Unsigned(cursor.u32("VT_UI4")? as u64),
        0x0015 => PropertyValue::Unsigned(cursor.u64("VT_UI8")?),
        0x001e | 0x0008 => {
            let size = cursor.count("code-page string size", MAX_STREAM_SIZE)?;
            let value = cursor.code_page_string(ctx, size, code_page, "string")?;
            cursor.align4("string padding")?;
            PropertyValue::String(value)
        }
        0x001f => {
            let count = cursor.count("Unicode string length", MAX_STREAM_SIZE / 2)?;
            let value = cursor.unicode_string(ctx, count, "Unicode string")?;
            cursor.align4("Unicode string padding")?;
            PropertyValue::String(value)
        }
        0x0040 => PropertyValue::Filetime(cursor.u64("FILETIME")?),
        0x0041 | 0x0046 => {
            let size = cursor.count("BLOB size", MAX_STREAM_SIZE)?;
            let start = cursor.position();
            cursor.take(size, "BLOB")?;
            let value = PropertyValue::Binary(child(raw, start, cursor.position(), "BLOB")?);
            cursor.align4("BLOB padding")?;
            value
        }
        0x0047 => {
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
                format,
                data: child(raw, start, cursor.position(), "clipboard data")?,
            };
            cursor.align4("clipboard padding")?;
            value
        }
        0x0048 => PropertyValue::Guid(cursor.array("CLSID")?),
        _ => {
            cursor.skip_to_end();
            PropertyValue::Unknown
        }
    };
    if padded && !matches!(type_code, 0x0000 | 0x0001) {
        cursor.align4("scalar padding")?;
    }
    Ok(value)
}

fn child<'a>(raw: View<'a>, start: usize, end: usize, field: &str) -> Result<View<'a>, CodecError> {
    raw.child(raw.start() + start, raw.start() + end)
        .ok_or_else(|| CodecError::malformed(format_args!("OLE {field} view is invalid")))
}

fn require_zero_range(
    bytes: &[u8],
    start: usize,
    end: usize,
    field: &str,
) -> Result<(), CodecError> {
    let gap = bytes.get(start..end).ok_or_else(|| {
        CodecError::malformed(format_args!("OLE property-set {field} is invalid"))
    })?;
    if gap.iter().any(|byte| *byte != 0) {
        return Err(CodecError::malformed(format_args!(
            "OLE property-set {field} is nonzero"
        )));
    }
    Ok(())
}

fn decode_code_page(bytes: &[u8], code_page: Option<u16>) -> Result<String, CodecError> {
    if code_page == Some(1200) {
        if !bytes.len().is_multiple_of(2) {
            return Err(CodecError::Malformed(
                "OLE Unicode code-page string has an odd byte length".into(),
            ));
        }
        let mut view = View::over_retained(bytes);
        let value = view
            .utf16_le(bytes.len() / 2)
            .ok_or_else(|| CodecError::Malformed("OLE code-page string is not UTF-16".into()))?;
        return require_and_remove_null(value, "OLE Unicode code-page string");
    }
    let (content, had_null) = bytes
        .strip_suffix(&[0])
        .map_or((bytes, false), |content| (content, true));
    if !bytes.is_empty() && !had_null {
        return Err(CodecError::Malformed(
            "OLE code-page string has no null terminator".into(),
        ));
    }
    let encoding = encoding_for_code_page(code_page.unwrap_or(1252)).ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "OLE code page {} is not implemented",
            code_page.unwrap_or(1252)
        ))
    })?;
    let (decoded, _, malformed) = encoding.decode(content);
    if malformed {
        return Err(CodecError::malformed(format_args!(
            "OLE code-page {} string is malformed",
            code_page.unwrap_or(1252)
        )));
    }
    Ok(decoded.into_owned())
}

fn encoding_for_code_page(code_page: u16) -> Option<&'static encoding_rs::Encoding> {
    let label = match code_page {
        65001 => "utf-8",
        874 => "windows-874",
        932 => "shift_jis",
        936 => "gbk",
        949 => "euc-kr",
        950 => "big5",
        1250 => "windows-1250",
        1251 => "windows-1251",
        1252 => "windows-1252",
        1253 => "windows-1253",
        1254 => "windows-1254",
        1255 => "windows-1255",
        1256 => "windows-1256",
        1257 => "windows-1257",
        1258 => "windows-1258",
        _ => return None,
    };
    encoding_rs::Encoding::for_label(label.as_bytes())
}

fn require_and_remove_null(value: String, field: &str) -> Result<String, CodecError> {
    if value.is_empty() {
        return Ok(value);
    }
    value
        .strip_suffix('\0')
        .map(str::to_owned)
        .ok_or_else(|| CodecError::malformed(format_args!("{field} has no null terminator")))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
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
        self.view.position().saturating_sub(self.view.start())
    }

    fn skip_to_end(&mut self) {
        let _ = self.view.seek(self.view.end());
    }

    fn truncated(&self, field: &str) -> CodecError {
        CodecError::malformed(format_args!("truncated {} {field}", self.scope))
    }

    fn take(&mut self, len: usize, field: &str) -> Result<&'a [u8], CodecError> {
        if self.view.position().checked_add(len).is_none() {
            return Err(CodecError::malformed(format_args!(
                "{} {field} range overflows",
                self.scope
            )));
        }
        self.view.take(len).ok_or_else(|| self.truncated(field))
    }

    fn u8(&mut self, field: &str) -> Result<u8, CodecError> {
        self.view.u8().ok_or_else(|| self.truncated(field))
    }

    fn u16(&mut self, field: &str) -> Result<u16, CodecError> {
        self.view.u16_le().ok_or_else(|| self.truncated(field))
    }

    fn i16(&mut self, field: &str) -> Result<i16, CodecError> {
        self.view.i16_le().ok_or_else(|| self.truncated(field))
    }

    fn u32(&mut self, field: &str) -> Result<u32, CodecError> {
        self.view.u32_le().ok_or_else(|| self.truncated(field))
    }

    fn i32(&mut self, field: &str) -> Result<i32, CodecError> {
        self.view.i32_le().ok_or_else(|| self.truncated(field))
    }

    fn u64(&mut self, field: &str) -> Result<u64, CodecError> {
        self.view.u64_le().ok_or_else(|| self.truncated(field))
    }

    fn i64(&mut self, field: &str) -> Result<i64, CodecError> {
        self.view.i64_le().ok_or_else(|| self.truncated(field))
    }

    fn array<const N: usize>(&mut self, field: &str) -> Result<[u8; N], CodecError> {
        self.view.array().ok_or_else(|| self.truncated(field))
    }

    fn count(&mut self, field: &str, maximum: usize) -> Result<usize, CodecError> {
        let value = self.offset(field)?;
        if value > maximum {
            return Err(CodecError::malformed(format_args!(
                "{} {field} exceeds {maximum}",
                self.scope
            )));
        }
        Ok(value)
    }

    fn offset(&mut self, field: &str) -> Result<usize, CodecError> {
        usize::try_from(self.u32(field)?)
            .map_err(|_| CodecError::malformed(format_args!("{} {field} is too large", self.scope)))
    }

    fn align4(&mut self, field: &str) -> Result<(), CodecError> {
        let padding = (4 - self.position() % 4) % 4;
        if self.take(padding, field)?.iter().any(|byte| *byte != 0) {
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
        field: &str,
    ) -> Result<String, CodecError> {
        let byte_len = if code_page == Some(1200) {
            size.checked_mul(2).ok_or_else(|| {
                CodecError::malformed(format_args!("{} {field} length overflows", self.scope))
            })?
        } else {
            size
        };
        ctx.charge_retained(byte_len as u64, "retain OLE property string", None)?;
        decode_code_page(self.take(byte_len, field)?, code_page)
    }

    fn unicode_string(
        &mut self,
        ctx: &DecodeContext<'_>,
        count: usize,
        field: &str,
    ) -> Result<String, CodecError> {
        let byte_len = count.checked_mul(2).ok_or_else(|| {
            CodecError::malformed(format_args!("{} {field} length overflows", self.scope))
        })?;
        ctx.charge_retained(byte_len as u64, "retain OLE Unicode property string", None)?;
        let value = self.view.utf16_le(count).ok_or_else(|| {
            if self.view.remaining() < byte_len {
                self.truncated(field)
            } else {
                CodecError::malformed(format_args!("{} {field} is not UTF-16", self.scope))
            }
        })?;
        require_and_remove_null(value, field)
    }

    fn zero_finish(self) -> Result<(), CodecError> {
        if self
            .view
            .window()
            .get(self.position()..)
            .is_some_and(|rest| rest.iter().all(|byte| *byte == 0))
        {
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
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    use super::*;

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
            PropertyValue::String(value) if value == "Synthetic title"
        ));
        assert!(matches!(
            &section.properties[2].value,
            PropertyValue::Binary(value) if value.window().starts_with(b"\x89PNG\r\n\x1a\n")
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
        bytes.extend_from_slice(&(section_size as u32).to_le_bytes());
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        for (id, offset) in [
            (1_u32, code_page_offset),
            (2, title_offset),
            (17, preview_offset),
        ] {
            bytes.extend_from_slice(&id.to_le_bytes());
            bytes.extend_from_slice(&(offset as u32).to_le_bytes());
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
        bytes.extend_from_slice(&(units.len() as u32).to_le_bytes());
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
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
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
