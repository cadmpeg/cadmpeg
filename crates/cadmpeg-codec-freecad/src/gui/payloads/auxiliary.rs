// SPDX-License-Identifier: Apache-2.0
//! Producer-selected f32 arrays and the six mesh-material arrays of an entry.

use super::{finite32, framed_spec, spec, PayloadProperty, SourceRequest};
use crate::native::entry_contents::{EntryContentUpdate, EntryContents, EntryGrammar, F32Values};
use cadmpeg_core::decode::{DecodeContext, RecordOwner, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteBinary32;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};

pub(crate) fn width(kind: &str) -> Option<usize> {
    match kind {
        "Points::PropertyGreyValueList" | "Inspection::PropertyDistanceList" => Some(1),
        "Points::PropertyNormalList" | "Mesh::PropertyNormalList" => Some(3),
        "Points::PropertyCurvatureList" | "Mesh::PropertyCurvatureList" => Some(8),
        _ => None,
    }
}

pub(super) fn decode<'source>(
    ctx: &DecodeContext<'source>,
    property: PayloadProperty<'_>,
    request: SourceRequest<'_, 'source>,
    memo: &mut super::sources::EntryInterpretations<'source>,
    output: &mut PartOutput,
) -> Result<super::sources::DecodedPayloads<'source>, CodecError> {
    let components = width(property.type_name);
    if components.is_none() && property.type_name != "Mesh::PropertyMaterial" {
        return Ok(Default::default());
    }
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
    let id = crate::native::native_id("entry", name);
    let grammar = components.map_or(EntryGrammar::MeshMaterial, EntryGrammar::F32);
    if let Some(alias) = memo.find(ctx, &id, grammar, entry)?.or(request
        .base
        .map(|base| base.find(ctx, &id, grammar, entry))
        .transpose()?
        .flatten())
    {
        ctx.record_fragment(owner, super::control_extent(&request)?, || {
            super::read_control(ctx, &request, name, 0, property.type_name)
        })?;
        let mut result = super::sources::DecodedPayloads::default();
        if let Some(binding) = &alias.binding {
            result.sources.import_alias(ctx, property.id, binding)?;
        }
        return Ok(result);
    }
    let level = if components.is_some() {
        "freecad.f32_record_list_entry"
    } else {
        "freecad.mesh_material_entry"
    };
    let mut entry_spec = if components.is_some() {
        framed_spec(level, entry, &[(0, 4)])?
    } else {
        spec(level)?
    };
    if let Some(parent) = request.parents.and_then(|parents| parents.get(name)) {
        entry_spec = entry_spec.with_parent(parent);
    }
    let contents = ctx.decode_part(output, entry, entry_spec, |stage| {
        ctx.record_fragment(owner, super::control_extent(&request)?, || {
            super::read_control(ctx, &request, name, 0, property.type_name)
        })?;
        let mut cursor = entry;
        let mut sources = super::sources::PayloadSources::default();
        let contents = if let Some(components) = components {
            f32_values(
                ctx,
                &mut cursor,
                components,
                owner,
                stage,
                &property,
                &mut sources,
            )?
        } else {
            mesh_material(ctx, &mut cursor, owner, stage, property.id, &mut sources)?
        };
        if !cursor.is_empty() {
            return Err(CodecError::malformed("auxiliary entry has trailing bytes"));
        }
        Ok((contents, sources))
    })?;
    let mut result = super::sources::DecodedPayloads::default();
    let mut failed = None;
    let actual = ctx.capture_part_extent(entry)?;
    if let Some((contents, sources)) = contents {
        result.sources = sources;
        result
            .entry_contents
            .push(EntryContentUpdate::new(ctx, &id, entry, contents)?);
    } else {
        failed = output
            .fidelity()
            .parts()
            .iter()
            .find(|(_, part)| {
                part.stream() == actual.stream()
                    && part.byte_len() == actual.byte_len()
                    && part.level().as_str() == level
                    && part.offset() == cadmpeg_core::decode::u64_from_index(entry.start())
                    && matches!(
                        part.state(),
                        cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                    )
            })
            .map(|(id, _)| id.as_str().to_owned());
    }
    let binding = result.sources.export_alias(ctx, property.id)?;
    memo.insert(ctx, &id, grammar, entry, binding, failed)?;
    Ok(result)
}
fn row_extent<'source>(
    cursor: &mut View<'source>,
    width: usize,
) -> Result<(View<'source>, bool), CodecError> {
    let length = cursor.remaining().min(width);
    let start = cursor.position();
    let end = start
        .checked_add(length)
        .ok_or_else(|| CodecError::malformed("auxiliary row extent overflows"))?;
    let row = cursor
        .child(start, end)
        .ok_or_else(|| CodecError::malformed("auxiliary row escapes entry"))?;
    cursor
        .skip(length)
        .ok_or_else(|| CodecError::malformed("auxiliary skip escapes entry"))?;
    Ok((row, length == width))
}
fn f32_values<'source>(
    ctx: &DecodeContext<'source>,
    cursor: &mut View<'source>,
    width: usize,
    owner: &RecordOwner<'_>,
    output: &mut PartOutput,
    property: &PayloadProperty<'_>,
    sources: &mut super::sources::PayloadSources<'source>,
) -> Result<EntryContents, CodecError> {
    let header = cursor
        .child(
            cursor.position(),
            cursor
                .position()
                .checked_add(cursor.remaining().min(4))
                .ok_or_else(|| CodecError::malformed("f32 header end overflows"))?,
        )
        .ok_or_else(|| CodecError::malformed("f32 header escapes source"))?;
    let count = ctx.record_fragment(owner, header, || {
        usize::try_from(cursor.req_u32_le()?).map_err(CodecError::malformed)
    })?;
    sources.header_source(ctx, property.id, property.type_name, *cursor, header)?;
    let mut values = match width {
        1 => F32Values::Scalar(crate::native::entry_contents::alloc_slots(
            ctx,
            count,
            None,
            "entry f32 scalar slots",
        )?),
        3 => F32Values::Vector(crate::native::entry_contents::alloc_slots(
            ctx,
            count,
            None,
            "entry f32 vector slots",
        )?),
        8 => F32Values::Curvature(crate::native::entry_contents::alloc_slots(
            ctx,
            count,
            None,
            "entry f32 curvature slots",
        )?),
        _ => return Err(CodecError::malformed("unsupported f32 record width")),
    };
    for index in 0..count {
        let (row, complete) = row_extent(
            cursor,
            width
                .checked_mul(4)
                .ok_or_else(|| CodecError::malformed("f32 row width overflows"))?,
        )?;
        let value = ctx.record_fragment(owner, row, || {
            ctx.decode_part(output, row, spec("freecad.f32_record")?, |_| {
                let mut source = row;
                let mut value = [FiniteBinary32::new(0.0)
                    .ok_or_else(|| CodecError::malformed("zero is not finite"))?;
                    8];
                for component in value.iter_mut().take(width) {
                    *component = finite32(&mut source)?;
                }
                Ok(value)
            })
        })?;
        if let Some(value) = value {
            sources.row_source(ctx, property.id, property.type_name, index, row)?;
            match &mut values {
                F32Values::Scalar(values) => values[index] = Some(value[0]),
                F32Values::Vector(values) => values[index] = Some([value[0], value[1], value[2]]),
                F32Values::Curvature(values) => values[index] = Some(value),
            }
        }
        if !complete {
            break;
        }
    }
    Ok(EntryContents::F32 { values })
}
fn mesh_material<'source>(
    ctx: &DecodeContext<'source>,
    cursor: &mut View<'source>,
    owner: &RecordOwner<'_>,
    output: &mut PartOutput,
    property: &str,
    sources: &mut super::sources::PayloadSources<'source>,
) -> Result<EntryContents, CodecError> {
    let (binding_view, binding_complete) = row_extent(cursor, 4)?;
    let binding = ctx.record_fragment(owner, binding_view, || {
        ctx.decode_part(
            output,
            binding_view,
            spec("freecad.mesh_material_binding")?,
            |_| {
                let mut value = binding_view;
                let value = value.req_u32_le()?;
                if value > 2 {
                    return Err(CodecError::malformed("invalid mesh material binding"));
                }
                Ok(value)
            },
        )
    })?;
    let failed = if binding.is_none() {
        let extent = ctx.capture_part_extent(binding_view)?;
        output
            .fidelity()
            .parts()
            .iter()
            .find(|(_, part)| {
                part.level().as_str() == "freecad.mesh_material_binding"
                    && part.stream() == extent.stream()
                    && part.offset() == extent.offset()
                    && part.byte_len() == extent.byte_len()
                    && matches!(
                        part.state(),
                        cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                    )
            })
            .map(|(id, _)| id.as_str().to_owned())
    } else {
        None
    };
    sources.mesh_binding_source(ctx, property, binding_view, failed.as_deref())?;
    let mut ambient = None;
    let mut diffuse = None;
    let mut specular = None;
    let mut emissive = None;
    let mut shininess = None;
    let mut transparency = None;
    if binding_complete {
        for role in 0..6 {
            let start = cursor.position();
            let end = View::u32_le_at(cursor.unread(), 0)
                .and_then(|count| usize::try_from(count).ok())
                .and_then(|count| count.checked_mul(4))
                .and_then(|length| start.checked_add(4)?.checked_add(length));
            let selected = end.filter(|end| *end <= cursor.end());
            let extent = cursor
                .child(start, selected.unwrap_or(cursor.end()))
                .ok_or_else(|| CodecError::malformed("mesh material array escapes entry"))?;
            cursor
                .skip(extent.remaining())
                .ok_or_else(|| CodecError::malformed("mesh material array skip escapes entry"))?;
            let parsed = ctx.decode_part(
                output,
                extent,
                framed_spec("freecad.mesh_material_array", extent, &[(0, 4)])?,
                |stage| {
                    let mut array = extent;
                    let header = extent
                        .child(
                            extent.start(),
                            extent
                                .start()
                                .checked_add(extent.remaining().min(4))
                                .ok_or_else(|| {
                                    CodecError::malformed("mesh material count end overflows")
                                })?,
                        )
                        .ok_or_else(|| {
                            CodecError::malformed("mesh material count escapes source")
                        })?;
                    let count = ctx.record_fragment(owner, header, || {
                        usize::try_from(array.req_u32_le()?).map_err(CodecError::malformed)
                    })?;
                    if role < 4 {
                        let mut values = crate::native::entry_contents::alloc_slots(
                            ctx,
                            count,
                            None,
                            "entry mesh color slots",
                        )?;
                        let mut source_fields =
                            super::sources::mesh_material::Array::new(ctx, header)?;
                        for (ordinal, value) in values.iter_mut().enumerate() {
                            let (row, complete) = row_extent(&mut array, 4)?;
                            *value = ctx.record_fragment(owner, row, || {
                                ctx.decode_part(
                                    stage,
                                    row,
                                    spec("freecad.mesh_material_value")?,
                                    |_| {
                                        let mut value = row;
                                        Ok(value.req_u32_le()?)
                                    },
                                )
                            })?;
                            if value.is_some() {
                                source_fields.row(ctx, ordinal, row)?;
                            }
                            if !complete {
                                break;
                            }
                        }
                        Ok((MeshValues::Color(values), source_fields))
                    } else {
                        let mut values = crate::native::entry_contents::alloc_slots(
                            ctx,
                            count,
                            None,
                            "entry mesh scalar slots",
                        )?;
                        let mut source_fields =
                            super::sources::mesh_material::Array::new(ctx, header)?;
                        for (ordinal, value) in values.iter_mut().enumerate() {
                            let (row, complete) = row_extent(&mut array, 4)?;
                            *value = ctx.record_fragment(owner, row, || {
                                ctx.decode_part(
                                    stage,
                                    row,
                                    spec("freecad.mesh_material_value")?,
                                    |_| {
                                        let mut value = row;
                                        finite32(&mut value)
                                    },
                                )
                            })?;
                            if value.is_some() {
                                source_fields.row(ctx, ordinal, row)?;
                            }
                            if !complete {
                                break;
                            }
                        }
                        Ok((MeshValues::Float(values), source_fields))
                    }
                },
            )?;
            let parsed = parsed
                .map(|(value, fields)| {
                    sources.mesh_array_source(ctx, property, role, fields)?;
                    Ok::<_, CodecError>(value)
                })
                .transpose()?;
            match (role, parsed) {
                (0, Some(MeshValues::Color(values))) => ambient = Some(values),
                (1, Some(MeshValues::Color(values))) => diffuse = Some(values),
                (2, Some(MeshValues::Color(values))) => specular = Some(values),
                (3, Some(MeshValues::Color(values))) => emissive = Some(values),
                (4, Some(MeshValues::Float(values))) => shininess = Some(values),
                (5, Some(MeshValues::Float(values))) => transparency = Some(values),
                (_, None) => {}
                _ => {
                    return Err(CodecError::malformed(
                        "mesh material array differs from its position",
                    ))
                }
            }
            if selected.is_none() {
                break;
            }
        }
    }
    Ok(EntryContents::MeshMaterial {
        binding,
        ambient,
        diffuse,
        specular,
        emissive,
        shininess,
        transparency,
    })
}
enum MeshValues {
    Color(Vec<Option<u32>>),
    Float(Vec<Option<FiniteBinary32>>),
}
