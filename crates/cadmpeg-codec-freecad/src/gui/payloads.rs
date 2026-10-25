// SPDX-License-Identifier: Apache-2.0
//! Binary property interpretations owned by their genuine archive entry.

pub(crate) mod auxiliary;
mod material;
pub(crate) mod projection;
pub(super) use projection::{material_lists, MaterialList};
pub(crate) mod sources;
use crate::native::entry_contents::{
    BinaryPrecision, EntryContentUpdate, EntryContents, EntryGrammar, FilletValue,
};
use crate::native::{GuiPropertyRecord, PropertyRecord, ValueRecord};
use crate::xml_framing::XmlSource;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{FiniteBinary32, FiniteReal};
use cadmpeg_ir::source_fidelity::parts::{
    DecodeParts, PartLevel, PartOutput, PartParent, PartSpec,
};
use std::collections::BTreeMap;

pub(crate) struct PayloadProperty<'a> {
    pub(crate) id: &'a str,
    pub(crate) type_name: &'a str,
    pub(crate) values: &'a [ValueRecord],
    pub(crate) side_entries: &'a [String],
}
impl<'a> From<&'a GuiPropertyRecord> for PayloadProperty<'a> {
    fn from(p: &'a GuiPropertyRecord) -> Self {
        Self {
            id: &p.id,
            type_name: &p.type_name,
            values: &p.values,
            side_entries: &p.side_entries,
        }
    }
}
impl<'a> From<&'a PropertyRecord> for PayloadProperty<'a> {
    fn from(p: &'a PropertyRecord) -> Self {
        Self {
            id: &p.id,
            type_name: &p.type_name,
            values: p.values(),
            side_entries: p.side_entries(),
        }
    }
}

pub(crate) struct SourceRequest<'a, 'source> {
    pub(crate) xml: &'a XmlSource<'source>,
    pub(crate) value: usize,
    pub(crate) entries: &'a BTreeMap<String, View<'source>>,
    pub(crate) parents: Option<&'a BTreeMap<String, PartParent>>,
    pub(crate) owners: Option<&'a crate::container::parts::EntryOwners<'source>>,
    pub(crate) base: Option<&'a sources::EntryInterpretations<'source>>,
}

pub(crate) fn selected_attribute(
    ctx: &DecodeContext<'_>,
    xml: &XmlSource<'_>,
    index: usize,
    name: &str,
) -> Result<Option<String>, CodecError> {
    for key in xml.attribute_names(index)?.into_iter().rev() {
        if key != "xmlns" && !key.starts_with("xmlns:") && key.rsplit(':').next() == Some(name) {
            return xml.attribute(ctx, index, key);
        }
    }
    Ok(None)
}

fn runtime(kind: &str) -> Option<(&'static str, &'static str, usize)> {
    Some(match kind {
        "App::PropertyColorList" => ("freecad.color_list_entry", "freecad.color_list_color", 4),
        "App::PropertyFloatList" => ("freecad.float_list_entry", "freecad.float_list_value", 8),
        "App::PropertyVectorList" => (
            "freecad.vector_list_entry",
            "freecad.vector_list_vector",
            24,
        ),
        "App::PropertyPlacementList" => (
            "freecad.placement_list_entry",
            "freecad.placement_list_placement",
            56,
        ),
        "Part::PropertyFilletEdges" => ("freecad.fillet_edges_entry", "freecad.fillet_edge", 20),
        "App::PropertyMaterialList" => (
            "freecad.material_list_entry",
            "freecad.material_list_material",
            24,
        ),
        _ => return None,
    })
}

pub(crate) fn decode<'source>(
    ctx: &DecodeContext<'source>,
    property: PayloadProperty<'_>,
    request: SourceRequest<'_, 'source>,
    memo: &mut sources::EntryInterpretations<'source>,
    output: &mut PartOutput,
) -> Result<sources::DecodedPayloads<'source>, CodecError> {
    if !property.values.iter().any(|value| {
        value.parent.is_none()
            && value
                .attributes
                .get("file")
                .is_some_and(|file| !file.is_empty())
    }) {
        return Ok(Default::default());
    }
    let Some((entry_level, row_level, default_size)) = runtime(property.type_name) else {
        return auxiliary::decode(ctx, property, request, memo, output);
    };
    let peek = crate::persistence::source::peek_value_attributes(ctx, request.xml, request.value)?;
    let Some(name) = peek.get("file").filter(|name| !name.is_empty()) else {
        return Ok(Default::default());
    };
    let Some(entry) = request.entries.get(name).copied() else {
        return Ok(Default::default());
    };
    let Some(owner) = request.owners.and_then(|owners| owners.get(name)) else {
        return Ok(Default::default());
    };
    let entry_id = crate::native::native_id("entry", name);
    let version = if property.type_name == "App::PropertyMaterialList" {
        peek.get("version")
            .map(|value| value.parse::<u32>().map_err(CodecError::malformed))
            .transpose()?
            .unwrap_or(0)
    } else {
        0
    };
    let header = if property.type_name == "App::PropertyMaterialList"
        && version <= 1
        && View::u32_le_at(entry.window(), 0).is_some_and(|word| word & 0x8000_0000 != 0)
    {
        8
    } else {
        4
    };
    let count = View::u32_le_at(entry.window(), header - 4);
    let single = matches!(
        property.type_name,
        "App::PropertyVectorList" | "App::PropertyPlacementList"
    ) && count
        .filter(|count| *count != 0)
        .and_then(|count| usize::try_from(count).ok())
        .and_then(|count| count.checked_mul(default_size / 2))
        .and_then(|size| size.checked_add(header))
        == Some(entry.remaining());
    let size = if single {
        default_size / 2
    } else {
        default_size
    };
    let precision = if single {
        BinaryPrecision::Single
    } else {
        BinaryPrecision::Double
    };
    let grammar = match property.type_name {
        "App::PropertyColorList" => EntryGrammar::Color,
        "App::PropertyFloatList" => EntryGrammar::Float,
        "App::PropertyVectorList" => EntryGrammar::Vector(precision),
        "App::PropertyPlacementList" => EntryGrammar::Placement(precision),
        "Part::PropertyFilletEdges" => EntryGrammar::Fillet,
        _ => EntryGrammar::Material(version),
    };
    let alias = memo.find(ctx, &entry_id, grammar, entry)?.or(request
        .base
        .map(|base| base.find(ctx, &entry_id, grammar, entry))
        .transpose()?
        .flatten());
    if let Some(alias) = alias {
        let failed_alias = alias.given_up.is_some();
        ctx.record_fragment(owner, control_extent(&request)?, || {
            read_control(ctx, &request, name, version, property.type_name)
        })?;
        let mut decoded = sources::DecodedPayloads::default();
        if let Some(binding) = &alias.binding {
            decoded.sources.import_alias(ctx, property.id, binding)?;
        }
        if !failed_alias {
            bind_material_name(ctx, property.id, &request, memo)?;
        }
        return Ok(decoded);
    }
    let count_width = if matches!(
        property.type_name,
        "App::PropertyVectorList" | "App::PropertyPlacementList"
    ) {
        crate::layout::link_array_side_entry_header::LEN
    } else {
        header
    };
    let mut entry_spec = framed_spec(entry_level, entry, &[(0, count_width)])?;
    if let Some(parent) = request.parents.and_then(|parents| parents.get(name)) {
        entry_spec = entry_spec.with_parent(parent);
    }
    let parsed = ctx.decode_part(output, entry, entry_spec, |stage| {
        ctx.record_fragment(owner, control_extent(&request)?, || {
            read_control(ctx, &request, name, version, property.type_name)
        })?;
        let mut cursor = entry;
        let header_view = entry
            .child(
                entry.start(),
                entry
                    .start()
                    .checked_add(header.min(entry.remaining()))
                    .ok_or_else(|| CodecError::malformed("entry header end overflows"))?,
            )
            .ok_or_else(|| CodecError::malformed("entry count extent escapes source"))?;
        let count = ctx.record_fragment(owner, header_view, || {
            let count = if header == 8 {
                cursor.req_i32_le()?;
                cursor.req_u32_le()?
            } else {
                cursor.req_u32_le()?
            };
            usize::try_from(count).map_err(CodecError::malformed)
        })?;
        let mut source_fields = sources::PayloadSources::default();
        source_fields.header_source(ctx, property.id, property.type_name, entry, header_view)?;
        let mut material_strings = Vec::new();
        let contents = if property.type_name == "App::PropertyMaterialList" {
            let (contents, strings) =
                material::decode(ctx, &mut cursor, count, version, owner, stage)?;
            material_strings = strings;
            contents
        } else {
            let mut contents = empty(ctx, grammar, count)?;
            for ordinal in 0..count {
                let start = cursor.position();
                let available = cursor.remaining().min(size);
                let end = start
                    .checked_add(available)
                    .ok_or_else(|| CodecError::malformed("binary row extent overflows"))?;
                let row = cursor
                    .child(start, end)
                    .ok_or_else(|| CodecError::malformed("binary row escapes entry"))?;
                cursor
                    .skip(available)
                    .ok_or_else(|| CodecError::malformed("binary row skip escapes entry"))?;
                if let Some(value) = ctx.record_fragment(owner, row, || {
                    ctx.decode_part(stage, row, spec(row_level)?, |_| read_row(grammar, row))
                })? {
                    put(&mut contents, ordinal, value)?;
                    source_fields.row_source(ctx, property.id, property.type_name, ordinal, row)?;
                }
                if available < size {
                    break;
                }
            }
            contents
        };
        if !cursor.is_empty() {
            return Err(CodecError::malformed("binary entry has trailing bytes"));
        }
        Ok((contents, source_fields, material_strings))
    })?;
    let mut decoded = sources::DecodedPayloads::default();
    let mut failed = None;
    let actual = ctx.capture_part_extent(entry)?;
    let mut material_strings = Vec::new();
    if let Some((contents, source_fields, strings)) = parsed {
        material_strings = strings;
        decoded
            .entry_contents
            .push(EntryContentUpdate::new(ctx, &entry_id, entry, contents)?);
        decoded.sources = source_fields;
    } else {
        failed = output
            .fidelity()
            .parts()
            .iter()
            .find(|(_, part)| {
                part.stream() == actual.stream()
                    && part.level().as_str() == entry_level
                    && part.offset() == cadmpeg_core::decode::u64_from_index(entry.start())
                    && part.byte_len() == cadmpeg_core::decode::u64_from_index(entry.remaining())
                    && matches!(
                        part.state(),
                        cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                    )
            })
            .map(|(id, _)| id.as_str().to_owned());
    }
    let binding = decoded.sources.export_alias(ctx, property.id)?;
    memo.insert(ctx, &entry_id, grammar, entry, binding, failed)?;
    if property.type_name == "App::PropertyMaterialList" {
        memo.bind_material_strings(&entry_id, grammar, material_strings)?;
    }
    if !decoded.entry_contents.is_empty() {
        bind_material_name(ctx, property.id, &request, memo)?;
    }
    Ok(decoded)
}

fn bind_material_name<'source>(
    ctx: &DecodeContext<'_>,
    property: &str,
    request: &SourceRequest<'_, 'source>,
    memo: &mut sources::EntryInterpretations<'source>,
) -> Result<(), CodecError> {
    if !matches!(
        request.xml.name(request.value)?,
        "MaterialList" | "ColorList"
    ) {
        return Ok(());
    }
    if let Some(control) = sources::appearance::Control::new(ctx, request.xml, request.value)? {
        memo.bind_appearance(ctx, property, control)?;
    }
    if request.xml.name(request.value)? != "MaterialList" {
        return Ok(());
    }
    let mut parent = request.xml.elements()[request.value].parent();
    while let Some(index) = parent {
        if matches!(request.xml.name(index)?, "Property" | "_Property") {
            if let Some(source) = request.xml.attribute_source(ctx, index, "name")? {
                memo.bind_material_name(ctx, property, source)?;
            }
            break;
        }
        parent = request.xml.elements()[index].parent();
    }
    Ok(())
}

fn control_extent<'source>(
    request: &SourceRequest<'_, 'source>,
) -> Result<View<'source>, CodecError> {
    let mut parent = Some(request.value);
    while let Some(index) = parent {
        if matches!(request.xml.name(index)?, "Property" | "_Property") {
            return request.xml.extent(index);
        }
        parent = request.xml.elements()[index].parent();
    }
    Err(CodecError::malformed(
        "binary value has no owning property extent",
    ))
}

fn read_control(
    ctx: &DecodeContext<'_>,
    request: &SourceRequest<'_, '_>,
    expected: &str,
    version: u32,
    runtime: &str,
) -> Result<(), CodecError> {
    let mut parent = Some(request.value);
    while let Some(index) = parent {
        if matches!(request.xml.name(index)?, "Property" | "_Property") {
            let kind = request.xml.attribute(ctx, index, "type")?.ok_or_else(|| {
                CodecError::malformed("binary entry property has no runtime type")
            })?;
            if kind != runtime {
                return Err(CodecError::malformed(
                    "binary entry runtime changed during admission",
                ));
            }
            break;
        }
        parent = request.xml.elements()[index].parent();
    }
    let file = selected_attribute(ctx, request.xml, request.value, "file")?
        .ok_or_else(|| CodecError::malformed("binary entry carrier has no filename"))?;
    if file != expected {
        return Err(CodecError::malformed(
            "binary entry filename changed during admission",
        ));
    }
    if request.xml.name(request.value)? == "MaterialList" {
        let actual = selected_attribute(ctx, request.xml, request.value, "version")?
            .map(|value| value.parse::<u32>().map_err(CodecError::malformed))
            .transpose()?
            .unwrap_or(0);
        if actual != version || actual > 3 {
            return Err(CodecError::malformed("invalid material list version"));
        }
    }
    Ok(())
}

fn empty(
    ctx: &DecodeContext<'_>,
    grammar: EntryGrammar,
    count: usize,
) -> Result<EntryContents, CodecError> {
    Ok(match grammar {
        EntryGrammar::Color => EntryContents::Color {
            values: crate::native::entry_contents::alloc_slots(
                ctx,
                count,
                None,
                "entry color slots",
            )?,
        },
        EntryGrammar::Float => EntryContents::Float {
            values: crate::native::entry_contents::alloc_slots(
                ctx,
                count,
                None,
                "entry float slots",
            )?,
        },
        EntryGrammar::Vector(precision) => EntryContents::Vector {
            precision,
            values: crate::native::entry_contents::alloc_slots(
                ctx,
                count,
                None,
                "entry vector slots",
            )?,
        },
        EntryGrammar::Placement(precision) => EntryContents::Placement {
            precision,
            values: crate::native::entry_contents::alloc_slots(
                ctx,
                count,
                None,
                "entry placement slots",
            )?,
        },
        EntryGrammar::Fillet => EntryContents::Fillet {
            values: crate::native::entry_contents::alloc_slots(
                ctx,
                count,
                None,
                "entry fillet slots",
            )?,
        },
        _ => return Err(CodecError::malformed("non-scalar entry grammar")),
    })
}
enum Row {
    Color(u32),
    Float(FiniteReal),
    Vector([FiniteReal; 3]),
    Placement([FiniteReal; 7]),
    Fillet(FilletValue),
}
fn read_row(grammar: EntryGrammar, mut row: View<'_>) -> Result<Row, CodecError> {
    Ok(match grammar {
        EntryGrammar::Color => Row::Color(row.req_u32_le()?),
        EntryGrammar::Float => Row::Float(read_real(&mut row, false)?),
        EntryGrammar::Vector(precision) => Row::Vector([
            read_real(&mut row, precision == BinaryPrecision::Single)?,
            read_real(&mut row, precision == BinaryPrecision::Single)?,
            read_real(&mut row, precision == BinaryPrecision::Single)?,
        ]),
        EntryGrammar::Placement(precision) => {
            let mut value = [FiniteReal::new(0.0)
                .ok_or_else(|| CodecError::malformed("zero is not finite"))?;
                7];
            for component in &mut value {
                *component = read_real(&mut row, precision == BinaryPrecision::Single)?;
            }
            if crate::placement::placement_components(&value.map(FiniteReal::get)).is_none() {
                return Err(CodecError::malformed("invalid placement quaternion"));
            }
            Row::Placement(value)
        }
        EntryGrammar::Fillet => Row::Fillet(FilletValue {
            edge: row.req_i32_le()?,
            radius1: read_real(&mut row, false)?,
            radius2: read_real(&mut row, false)?,
        }),
        _ => return Err(CodecError::malformed("non-scalar row grammar")),
    })
}
fn put(contents: &mut EntryContents, index: usize, row: Row) -> Result<(), CodecError> {
    match (contents, row) {
        (EntryContents::Color { values }, Row::Color(value)) => values[index] = Some(value),
        (EntryContents::Float { values }, Row::Float(value)) => values[index] = Some(value),
        (EntryContents::Vector { values, .. }, Row::Vector(value)) => values[index] = Some(value),
        (EntryContents::Placement { values, .. }, Row::Placement(value)) => {
            values[index] = Some(value)
        }
        (EntryContents::Fillet { values }, Row::Fillet(value)) => values[index] = Some(value),
        _ => {
            return Err(CodecError::malformed(
                "row differs from its registered entry grammar",
            ))
        }
    }
    Ok(())
}
pub(crate) fn read_real(cursor: &mut View<'_>, single: bool) -> Result<FiniteReal, CodecError> {
    let value = if single {
        f64::from(cursor.req_f32_le()?)
    } else {
        cursor.req_f64_le()?
    };
    FiniteReal::new(value).ok_or_else(|| CodecError::malformed("non-finite binary real"))
}
pub(crate) fn finite32(cursor: &mut View<'_>) -> Result<FiniteBinary32, CodecError> {
    FiniteBinary32::new(cursor.req_f32_le()?)
        .ok_or_else(|| CodecError::malformed("non-finite binary float"))
}
pub(crate) fn spec(level: &str) -> Result<PartSpec, CodecError> {
    Ok(PartSpec::new(
        PartLevel::parse(level)
            .ok_or_else(|| CodecError::malformed("unregistered binary part level"))?,
        Vec::new(),
    ))
}
pub(crate) fn framed_spec(
    level: &str,
    view: View<'_>,
    fields: &[(usize, usize)],
) -> Result<PartSpec, CodecError> {
    let mut ranges = Vec::new();
    for &(start, len) in fields {
        let end = start
            .checked_add(len)
            .ok_or_else(|| CodecError::malformed("binary framing overflows"))?;
        if end <= view.remaining() {
            ranges.push(
                cadmpeg_core::decode::u64_from_index(view.start() + start)
                    ..cadmpeg_core::decode::u64_from_index(view.start() + end),
            );
        }
    }
    Ok(PartSpec::new(
        PartLevel::parse(level)
            .ok_or_else(|| CodecError::malformed("unregistered binary part level"))?,
        ranges,
    ))
}
