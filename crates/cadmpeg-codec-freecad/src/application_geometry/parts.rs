// SPDX-License-Identifier: Apache-2.0
//! Kernel rows are decoded from their actual fixed-width source parts.

use super::{association, point_transform, transform_point, ByteOrder, Reader};
use crate::gui::payloads::sources::EntryInterpretations;
use crate::native::entry_contents::{
    alloc_slots, EntryContentUpdate, EntryContents, EntryGrammar, MeshFacet,
};
use crate::native::{EntryRecord, GuiPropertyRecord, PropertyRecord, ValueRecord};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, RecordOwner, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::scalar::FiniteBinary32;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};

/// Borrowed actual property metadata shared by application and GUI owners.
#[derive(Clone, Copy)]
pub(crate) struct KernelProperty<'a> {
    pub(crate) id: &'a str,
    pub(crate) name: &'a str,
    pub(crate) type_name: &'a str,
    pub(crate) values: &'a [ValueRecord],
    pub(crate) object: Option<&'a str>,
}
impl<'a> From<&'a PropertyRecord> for KernelProperty<'a> {
    fn from(property: &'a PropertyRecord) -> Self {
        Self {
            id: &property.id,
            name: &property.name,
            type_name: &property.type_name,
            values: property.values(),
            object: Some(&property.owner),
        }
    }
}
impl<'a> From<&'a GuiPropertyRecord> for KernelProperty<'a> {
    fn from(property: &'a GuiPropertyRecord) -> Self {
        Self {
            id: &property.id,
            name: &property.name,
            type_name: &property.type_name,
            values: &property.values,
            object: None,
        }
    }
}
impl KernelProperty<'_> {
    fn mesh(self) -> Option<bool> {
        match self.type_name {
            "Mesh::PropertyMeshKernel" => Some(true),
            "Points::PropertyPointKernel" => Some(false),
            _ => None,
        }
    }
}

pub(crate) fn decode<'source>(
    ctx: &crate::property_reader::PropertyReader<'_, 'source>,
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
    sources: &crate::entry_sources::EntrySources<'source>,
    memo: &mut EntryInterpretations<'source>,
    output: &mut PartOutput,
) -> Result<Vec<EntryContentUpdate>, CodecError> {
    let mut updates = Vec::new();
    for source_property in properties {
        let property = KernelProperty::from(source_property);
        for value in property
            .values
            .iter()
            .filter(|value| value.parent.is_none())
        {
            let Some(name) = value.attributes.get("file").filter(|name| !name.is_empty()) else {
                continue;
            };
            let Some(entry) = entries.iter().find(|entry| entry.name() == name) else {
                continue;
            };
            let Some(view) = sources.optional_view(entry) else {
                continue;
            };
            if let Some(update) = decode_entry(
                ctx,
                KernelRequest {
                    property,
                    carrier: value,
                    name,
                    entry: entry.id(),
                    view,
                    parent: sources.parent(entry),
                    owner: sources.owner(entry)?,
                    base: None,
                },
                memo,
                |owner| {
                    let source = ctx.control_fields_into(
                        source_property,
                        value.order,
                        owner,
                        &["file", "mtrx"],
                    )?;
                    if source
                        .attributes
                        .get("file")
                        .is_none_or(|file| file != name)
                    {
                        return Err(CodecError::malformed(
                            "kernel source file reference differs",
                        ));
                    }
                    let mut value = value.clone();
                    value.attributes = source.attributes;
                    Ok(value)
                },
                output,
            )? {
                updates.push(update);
            }
        }
    }
    Ok(updates)
}

pub(crate) fn decode_gui<'source>(
    ctx: &DecodeContext<'source>,
    property: &GuiPropertyRecord,
    request: crate::gui::payloads::SourceRequest<'_, 'source>,
    memo: &mut EntryInterpretations<'source>,
    output: &mut PartOutput,
) -> Result<Vec<EntryContentUpdate>, CodecError> {
    let mut updates = Vec::new();
    let property = KernelProperty::from(property);
    if property.mesh().is_none() {
        return Ok(updates);
    }
    for value in property
        .values
        .iter()
        .filter(|value| value.parent.is_none())
    {
        let Some(name) = value.attributes.get("file").filter(|name| !name.is_empty()) else {
            continue;
        };
        let Some(view) = request.entries.get(name).copied() else {
            continue;
        };
        let owner = request
            .owners
            .and_then(|owners| owners.get(name))
            .ok_or_else(|| CodecError::malformed("kernel entry has no actual ZIP header owner"))?;
        let entry = crate::native::native_id("entry", name);
        if let Some(update) = decode_entry(
            ctx,
            KernelRequest {
                property,
                carrier: value,
                name,
                entry: &entry,
                view,
                parent: request.parents.and_then(|parents| parents.get(name)),
                owner,
                base: request.base,
            },
            memo,
            |owner| {
                let property_index =
                    request.xml.elements()[request.value]
                        .parent()
                        .ok_or_else(|| {
                            CodecError::malformed("GUI kernel value has no actual property")
                        })?;
                ctx.record_fragment(owner, request.xml.extent(property_index)?, || {
                    let read = |index, field| {
                        crate::gui::payloads::selected_attribute(ctx, request.xml, index, field)
                    };
                    if read(property_index, "name")?.as_deref() != Some(property.name)
                        || read(property_index, "type")?.as_deref() != Some(property.type_name)
                        || request.xml.read_name(request.value)? != value.tag
                    {
                        return Err(CodecError::malformed("GUI kernel source control differs"));
                    }
                    let mut value = value.clone();
                    value.attributes.clear();
                    for field in ["file", "mtrx"] {
                        if let Some(text) = read(request.value, field)? {
                            value.attributes.insert(field.to_owned(), text);
                        }
                    }
                    Ok(value)
                })
            },
            output,
        )? {
            updates.push(update);
        }
    }
    Ok(updates)
}

struct KernelRequest<'a, 'source> {
    property: KernelProperty<'a>,
    carrier: &'a ValueRecord,
    name: &'a str,
    entry: &'a str,
    view: View<'source>,
    parent: Option<&'a cadmpeg_ir::source_fidelity::parts::PartParent>,
    owner: &'a RecordOwner<'source>,
    base: Option<&'a EntryInterpretations<'source>>,
}

fn decode_entry<'source>(
    ctx: &DecodeContext<'source>,
    request: KernelRequest<'_, 'source>,
    memo: &mut EntryInterpretations<'source>,
    read_control: impl FnOnce(&RecordOwner<'_>) -> Result<ValueRecord, CodecError>,
    output: &mut PartOutput,
) -> Result<Option<EntryContentUpdate>, CodecError> {
    let KernelRequest {
        property,
        carrier,
        name,
        entry,
        view,
        parent,
        owner,
        base,
    } = request;
    let Some(mesh) = property.mesh() else {
        return Ok(None);
    };
    if carrier.tag != if mesh { "Mesh" } else { "Points" } {
        return Ok(None);
    }
    let grammars: &[EntryGrammar] = if mesh {
        &[EntryGrammar::Mesh(false), EntryGrammar::Mesh(true)]
    } else {
        &[EntryGrammar::Points]
    };
    for grammar in grammars {
        if memo
            .find(ctx, entry, *grammar, view)?
            .or(base
                .map(|base| base.find(ctx, entry, *grammar, view))
                .transpose()?
                .flatten())
            .is_some()
        {
            let carrier = read_control(owner)?;
            if carrier
                .attributes
                .get("file")
                .is_none_or(|file| file != name)
            {
                return Err(CodecError::malformed("kernel alias source file differs"));
            }
            return Ok(None);
        }
    }
    let (level, fields): (_, &[(usize, usize)]) = if mesh {
        (
            "freecad.mesh_kernel_entry",
            &[(0, 8), (super::mesh_hdr::LEN, 8)],
        )
    } else {
        ("freecad.point_kernel_entry", &[(0, 4)])
    };
    let spec = spec(view, level, fields)?;
    let spec = match parent {
        Some(parent) => spec.with_parent(parent),
        None => spec,
    };
    let parsed = ctx
        .decode_part(output, view, spec, |stage| {
            let carrier = read_control(owner)?;
            if carrier
                .attributes
                .get("file")
                .is_none_or(|file| file != name)
            {
                return Err(CodecError::malformed("kernel file reference differs"));
            }
            let value = if mesh {
                mesh_value(ctx, view, owner, stage)?
            } else {
                Some(points_value(ctx, view, owner, &[carrier], stage)?)
            };
            value
                .map(|value| {
                    let grammar = value.grammar();
                    EntryContentUpdate::new(ctx, entry, view, value).map(|update| (update, grammar))
                })
                .transpose()
        })
        .map(Option::flatten)?;
    let proof = ctx.source_extent_proof(view)?;
    let failed = if parsed.is_none() {
        output
            .fidelity()
            .parts()
            .iter()
            .find(|(_, part)| {
                part.level().as_str() == level
                    && part.stream() == proof.stream()
                    && part.offset() == u64_from_index(view.start())
                    && part.byte_len() == u64_from_index(view.window().len())
                    && matches!(
                        part.state(),
                        cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
                    )
            })
            .map(|(id, _)| id.as_str().to_owned())
    } else {
        None
    };
    let grammar = parsed.as_ref().map_or(grammars[0], |(_, grammar)| *grammar);
    memo.insert(ctx, entry, grammar, view, None, failed)?;
    Ok(parsed.map(|(update, _)| update))
}

pub(super) fn mesh_value(
    ctx: &DecodeContext<'_>,
    view: View<'_>,
    owner: &RecordOwner<'_>,
    output: &mut PartOutput,
) -> Result<Option<EntryContents>, CodecError> {
    let header = child(view, 0, view.window().len().min(super::mesh_hdr::LEN))?;
    let order = ctx.decode_part(
        output,
        header,
        spec(header, "freecad.mesh_kernel_header", &[(0, 8)])?,
        |_stage| {
            if header.window().len() != super::mesh_hdr::LEN {
                return Err(CodecError::malformed("mesh header is truncated"));
            }
            ctx.record_fragment(&owner, header, || {
                let mut reader = Reader { view: header };
                reader.mesh_byte_order("mesh")
            })
        },
    )?;
    let Some(order) = order else {
        return Ok(None);
    };
    let counts = child(view, super::mesh_hdr::LEN, 8)?;
    let (points, facets) = ctx.record_fragment(&owner, counts, || {
        let mut reader = Reader { view: counts };
        Ok((
            reader.count(order, "mesh point count")?,
            reader.count(order, "mesh facet count")?,
        ))
    })?;
    let point_bytes = points
        .checked_mul(12)
        .ok_or_else(|| CodecError::malformed("mesh point extent overflows"))?;
    let facet_bytes = facets
        .checked_mul(super::mesh_facet::LEN)
        .ok_or_else(|| CodecError::malformed("mesh facet extent overflows"))?;
    let point_start = super::mesh_hdr::LEN
        .checked_add(8)
        .ok_or_else(|| CodecError::malformed("mesh count extent overflows"))?;
    let facet_start = point_start
        .checked_add(point_bytes)
        .ok_or_else(|| CodecError::malformed("mesh facet start overflows"))?;
    let bounds_start = facet_start
        .checked_add(facet_bytes)
        .ok_or_else(|| CodecError::malformed("mesh bounds start overflows"))?;
    if bounds_start
        .checked_add(24)
        .is_some_and(|end| end < view.window().len())
    {
        return Err(CodecError::malformed(
            "mesh population exceeds remaining payload",
        ));
    }
    let mut vertices = alloc_slots(ctx, points, None, "FreeCAD mesh point slots")?;
    for (index, point) in vertices.iter_mut().enumerate() {
        let Some(extent) = partial_child(view, point_start + index * 12, 12)? else {
            break;
        };
        *point = ctx.decode_part(
            output,
            extent,
            spec(extent, "freecad.mesh_point", &[])?,
            |_stage| {
                ctx.record_fragment(&owner, extent, || {
                    Reader { view: extent }.point3(order, "mesh point")
                })
            },
        )?;
        if extent.window().len() < 12 {
            break;
        }
    }
    let mut triangles = alloc_slots(ctx, facets, None, "FreeCAD mesh facet slots")?;
    for (index, facet) in triangles.iter_mut().enumerate() {
        let Some(extent) = partial_child(
            view,
            facet_start + index * super::mesh_facet::LEN,
            super::mesh_facet::LEN,
        )?
        else {
            break;
        };
        *facet = ctx
            .decode_part(
                output,
                extent,
                spec(extent, "freecad.mesh_facet", &[])?,
                |_stage| {
                    ctx.record_fragment(&owner, extent, || {
                        let mut reader = Reader { view: extent };
                        let points = [
                            reader.index(order, points, "mesh facet point")?,
                            reader.index(order, points, "mesh facet point")?,
                            reader.index(order, points, "mesh facet point")?,
                        ];
                        let neighbours =
                            [reader.u32(order)?, reader.u32(order)?, reader.u32(order)?]
                                .map(|index| (index != u32::MAX).then_some(index));
                        if neighbours.iter().flatten().any(|index| {
                            usize::try_from(*index).map_or(true, |index| index >= facets)
                        }) {
                            return Err(CodecError::malformed(
                                "mesh facet neighbour is out of bounds",
                            ));
                        }
                        if points.iter().any(|index| {
                            usize::try_from(*index)
                                .ok()
                                .and_then(|index| vertices.get(index))
                                .is_none_or(Option::is_none)
                        }) {
                            return Ok(None);
                        }
                        Ok(Some(MeshFacet { points, neighbours }))
                    })
                },
            )?
            .flatten();
        if extent.window().len() < super::mesh_facet::LEN {
            break;
        }
    }
    let bounds = if let Some(extent) = partial_child(view, bounds_start, 24)? {
        ctx.decode_part(
            output,
            extent,
            spec(extent, "freecad.mesh_bounding_box", &[])?,
            |_stage| {
                ctx.record_fragment(&owner, extent, || {
                    let mut reader = Reader { view: extent };
                    let mut values = [FiniteBinary32::ZERO; 6];
                    for value in &mut values {
                        *value = FiniteBinary32::new(reader.f32(order)?).ok_or_else(|| {
                            CodecError::malformed("mesh bounding box contains a non-finite value")
                        })?;
                    }
                    Ok(values)
                })
            },
        )?
    } else {
        None
    };
    Ok(Some(EntryContents::Mesh {
        big_endian: matches!(order, ByteOrder::Big),
        vertices,
        facets: triangles,
        bounds,
    }))
}

pub(super) fn points_value(
    ctx: &DecodeContext<'_>,
    view: View<'_>,
    owner: &RecordOwner<'_>,
    values: &[ValueRecord],
    output: &mut PartOutput,
) -> Result<EntryContents, CodecError> {
    let transform = point_transform(values)?;
    let header = child(view, 0, 4)?;
    let count = ctx.record_fragment(owner, header, || {
        Reader { view: header }.count(ByteOrder::Little, "point-cloud point count")
    })?;
    if count
        .checked_mul(12)
        .and_then(|bytes| bytes.checked_add(4))
        .is_some_and(|end| end < view.window().len())
    {
        return Err(CodecError::malformed(
            "point-cloud count exceeds remaining payload",
        ));
    }
    let mut points = alloc_slots(ctx, count, None, "FreeCAD point-cloud slots")?;
    for (index, point) in points.iter_mut().enumerate() {
        let Some(extent) = partial_child(view, 4 + index * 12, 12)? else {
            break;
        };
        *point = ctx.decode_part(
            output,
            extent,
            spec(extent, "freecad.point_kernel_point", &[])?,
            |_stage| {
                ctx.record_fragment(&owner, extent, || {
                    let point =
                        Reader { view: extent }.point3(ByteOrder::Little, "point-cloud point")?;
                    transform_point(transform, point)?;
                    Ok(point)
                })
            },
        )?;
        if extent.window().len() < 12 {
            break;
        }
    }
    Ok(EntryContents::Points { points })
}

fn partial_child(
    view: View<'_>,
    offset: usize,
    length: usize,
) -> Result<Option<View<'_>>, CodecError> {
    let Some(available) = view.window().len().checked_sub(offset) else {
        return Ok(None);
    };
    child(view, offset, length.min(available)).map(Some)
}

fn child(view: View<'_>, offset: usize, length: usize) -> Result<View<'_>, CodecError> {
    view.start()
        .checked_add(offset)
        .and_then(|start| {
            start
                .checked_add(length)
                .and_then(|end| view.child(start, end))
        })
        .ok_or_else(|| CodecError::malformed("kernel part escapes the actual member"))
}

fn spec(view: View<'_>, level: &str, fields: &[(usize, usize)]) -> Result<PartSpec, CodecError> {
    let level = PartLevel::parse(level)
        .ok_or_else(|| CodecError::malformed("kernel part level is absent from Appendix C"))?;
    let framing = fields
        .iter()
        .filter(|&&(offset, length)| {
            offset
                .checked_add(length)
                .is_some_and(|end| end <= view.window().len())
        })
        .map(|&(offset, length)| {
            let field = child(view, offset, length)?;
            Ok(u64_from_index(field.start())..u64_from_index(field.end()))
        })
        .collect::<Result<_, CodecError>>()?;
    Ok(PartSpec::new(level, framing))
}

pub(crate) fn transfer(
    ctx: &crate::property_reader::PropertyReader<'_, '_>,
    ir: &mut cadmpeg_ir::CadIr,
    properties: &[PropertyRecord],
    admitted_entities: &mut u64,
    entry_sources: Option<&crate::entry_sources::EntrySources<'_>>,
    gui_source: Option<View<'_>>,
    gui_ranges: &std::collections::BTreeMap<String, crate::fidelity::ValueSourceRanges>,
) -> Result<bool, CodecError> {
    let entries = ir
        .native
        .namespace("fcstd")
        .map(|namespace| namespace.arena_as_for_decode::<EntryRecord>(ctx, "entries"))
        .transpose()?
        .unwrap_or_default();
    let namespace = ir.native.namespace("fcstd");
    let gui_properties = namespace
        .map(|ns| ns.arena_as_for_decode::<GuiPropertyRecord>(ctx, "gui_properties"))
        .transpose()?
        .unwrap_or_default();
    let providers = namespace
        .map(|ns| {
            ns.arena_as_for_decode::<crate::native::GuiViewProviderRecord>(
                ctx,
                "gui_view_providers",
            )
        })
        .transpose()?
        .unwrap_or_default();
    let extensions = namespace
        .map(|ns| ns.arena_as_for_decode::<crate::native::ExtensionRecord>(ctx, "extensions"))
        .transpose()?
        .unwrap_or_default();
    let gui_xml = gui_source
        .map(|view| crate::xml_framing::XmlSource::new(ctx, view))
        .transpose()?;
    let mut transferred = false;
    let all_properties =
        properties
            .iter()
            .map(KernelProperty::from)
            .chain(gui_properties.iter().map(|property| {
                let provider_owner = extensions
                    .iter()
                    .find(|extension| extension.id == property.owner)
                    .map_or(property.owner.as_str(), |extension| {
                        extension.owner.as_str()
                    });
                let object = providers
                    .iter()
                    .find(|provider| provider.id == provider_owner)
                    .and_then(|provider| provider.object.as_ref().map(|object| object.as_str()));
                KernelProperty {
                    object,
                    ..KernelProperty::from(property)
                }
            }));
    for property in all_properties {
        let application = properties
            .iter()
            .find(|candidate| candidate.id == property.id);
        let gui_supplier = gui_xml.as_ref().zip(gui_ranges.get(property.id));
        for (entry, value) in kernels_for_property(&entries, property) {
            let carrier = property
                .values
                .iter()
                .find(|carrier| {
                    carrier.parent.is_none()
                        && carrier
                            .attributes
                            .get("file")
                            .is_some_and(|file| file == entry.name())
                })
                .ok_or_else(|| CodecError::malformed("kernel has no held property carrier"))?;
            match value {
                EntryContents::Mesh {
                    vertices, facets, ..
                } => {
                    let id = format!("{}:mesh", property.id);
                    let mesh = super::sources::control(
                        ctx,
                        property,
                        application,
                        gui_supplier,
                        carrier,
                        &id,
                        |owner, control| {
                            if control
                                .attributes
                                .get("file")
                                .is_none_or(|file| file != entry.name())
                            {
                                return Err(CodecError::malformed(
                                    "projected mesh source file differs",
                                ));
                            }
                            let (vertices, facets) =
                                if let Some((owner, sources)) = owner.zip(entry_sources) {
                                    super::sources::mesh(
                                        ctx,
                                        owner,
                                        sources.view(entry)?,
                                        vertices,
                                        facets,
                                    )?
                                } else {
                                    (
                                        vertices.clone(),
                                        facets
                                            .iter()
                                            .map(|facet| facet.as_ref().map(|facet| facet.points))
                                            .collect(),
                                    )
                                };
                            project_mesh(ctx, property, vertices, facets)
                        },
                    )?;
                    let population = u64_from_index(ir.model.entity_count())
                        .checked_add(1)
                        .ok_or_else(|| CodecError::malformed("mesh entity count overflows"))?;
                    ctx.admit_entities(population, admitted_entities, "FreeCAD mesh entities")?;
                    ir.model
                        .tessellations
                        .push(mesh.with_native_ref(Some(entry.id().to_owned())));
                    transferred = true;
                }
                EntryContents::Points { points } => {
                    transferred |= project_points(
                        ctx,
                        points,
                        u64_from_index(ir.model.entity_count()),
                        admitted_entities,
                        |ordinal, point| {
                            let id =
                                crate::native::model_id("point", property.id, ordinal.to_string());
                            let point = super::sources::control(
                                ctx,
                                property,
                                application,
                                gui_supplier,
                                carrier,
                                &id,
                                |owner, control| {
                                    if control
                                        .attributes
                                        .get("file")
                                        .is_none_or(|file| file != entry.name())
                                    {
                                        return Err(CodecError::malformed(
                                            "projected point source file differs",
                                        ));
                                    }
                                    let point =
                                        if let Some((owner, sources)) = owner.zip(entry_sources) {
                                            super::sources::point(
                                                ctx,
                                                owner,
                                                sources.view(entry)?,
                                                ordinal,
                                                points.len(),
                                            )?
                                        } else {
                                            point
                                        };
                                    Ok(cadmpeg_ir::topology::Point::new(
                                        cadmpeg_ir::ids::PointId::mint(id.clone())
                                            .map_err(CodecError::malformed)?,
                                        transform_point(point_transform(&[control])?, point)?,
                                        property.object.map(association).transpose()?,
                                    )
                                    .with_native_ref(Some(entry.id().to_owned())))
                                },
                            )?;
                            Ok(point)
                        },
                        &mut ir.model.points,
                    )?;
                }
                _ => {}
            }
        }
    }
    Ok(transferred)
}

pub(super) fn project_mesh<'a>(
    ctx: &DecodeContext<'_>,
    property: impl Into<KernelProperty<'a>>,
    vertices: Vec<Option<FinitePoint3>>,
    facets: Vec<Option<[u32; 3]>>,
) -> Result<cadmpeg_ir::tessellation::Tessellation, CodecError> {
    let property = property.into();
    let mut dense = Vec::new();
    let mut indices = alloc_slots(
        ctx,
        vertices.len(),
        None,
        "FreeCAD mesh dense point mapping",
    )?;
    for (index, point) in vertices.into_iter().enumerate() {
        if let Some(point) = point {
            indices[index] = Some(
                u32::try_from(dense.len())
                    .map_err(|_| CodecError::malformed("mesh vertex index exceeds u32"))?,
            );
            dense.push(point);
        }
    }
    let triangles = facets
        .into_iter()
        .flatten()
        .filter_map(|facet| {
            let [a, b, c] = facet.map(|index| {
                usize::try_from(index)
                    .ok()
                    .and_then(|index| indices.get(index))
                    .copied()
                    .flatten()
            });
            Some([a?, b?, c?])
        })
        .collect();
    let mesh = cadmpeg_ir::tessellation::Tessellation::from_parts(
        cadmpeg_ir::tessellation::TessellationId::mint(format!("{}:mesh", property.id))
            .map_err(CodecError::malformed)?,
        cadmpeg_ir::tessellation::TessellationMesh::List {
            vertices: dense,
            triangles,
        },
        Vec::new(),
    )
    .map_err(CodecError::malformed)?
    .with_source_object(property.object.map(association).transpose()?);
    Ok(mesh)
}

pub(super) fn project_points(
    ctx: &crate::property_reader::PropertyReader<'_, '_>,
    points: &[Option<FinitePoint3>],
    current_entities: u64,
    admitted_entities: &mut u64,
    mut produce: impl FnMut(usize, FinitePoint3) -> Result<cadmpeg_ir::topology::Point, CodecError>,
    output: &mut Vec<cadmpeg_ir::topology::Point>,
) -> Result<bool, CodecError> {
    let count = points.iter().flatten().count();
    let population = current_entities
        .checked_add(u64_from_index(count))
        .ok_or_else(|| CodecError::malformed("point-cloud entity count overflows"))?;
    ctx.admit_entities(
        population,
        admitted_entities,
        "FreeCAD point-cloud entities",
    )?;
    for (ordinal, point) in points
        .iter()
        .enumerate()
        .filter_map(|(ordinal, point)| point.map(|point| (ordinal, point)))
    {
        output.push(produce(ordinal, point)?);
    }
    Ok(count != 0)
}

pub(crate) fn kernels_for_property<'a>(
    entries: &'a [EntryRecord],
    property: KernelProperty<'_>,
) -> Vec<(&'a EntryRecord, &'a EntryContents)> {
    let Some(mesh) = property.mesh() else {
        return Vec::new();
    };
    let tag = if mesh { "Mesh" } else { "Points" };
    property
        .values
        .iter()
        .filter(|value| value.parent.is_none() && value.tag == tag)
        .filter_map(|value| value.attributes.get("file"))
        .filter_map(|file| entries.iter().find(|entry| entry.name() == file))
        .flat_map(|entry| {
            entry.contents().iter().filter_map(move |value| {
                (matches!(value, EntryContents::Mesh { .. }) && mesh
                    || matches!(value, EntryContents::Points { .. }) && !mesh)
                    .then_some((entry, value))
            })
        })
        .collect()
}

pub(crate) fn validate(entries: &[EntryRecord]) -> Vec<(String, String)> {
    let mut findings = Vec::new();
    for entry in entries {
        for value in entry.contents() {
            if let EntryContents::Mesh {
                vertices, facets, ..
            } = value
            {
                if facets.iter().flatten().any(|facet| {
                    facet.points.iter().any(|index| {
                        usize::try_from(*index)
                            .ok()
                            .and_then(|index| vertices.get(index))
                            .is_none_or(Option::is_none)
                    }) || facet.neighbours.iter().flatten().any(|index| {
                        usize::try_from(*index).map_or(true, |index| index >= facets.len())
                    })
                }) {
                    findings.push((entry.id().to_owned(), "application mesh facet references an absent vertex or out-of-range neighbour".into()));
                }
            }
        }
    }
    findings
}
