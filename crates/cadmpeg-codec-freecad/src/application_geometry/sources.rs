// SPDX-License-Identifier: Apache-2.0
//! Independent source getters for projected kernel geometry.

use super::{ByteOrder, Reader};
use crate::native::entry_contents::{alloc_slots, MeshFacet};
use cadmpeg_core::decode::{DecodeContext, RecordOwner, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;

pub(super) fn mesh(
    ctx: &DecodeContext<'_>,
    owner: &RecordOwner<'_>,
    view: View<'_>,
    vertices: &[Option<FinitePoint3>],
    facets: &[Option<MeshFacet>],
) -> Result<(Vec<Option<FinitePoint3>>, Vec<Option<[u32; 3]>>), CodecError> {
    let (order, point_count, facet_count) = ctx.record_fragment(owner, view, || {
        let mut reader = Reader { view };
        let order = reader.mesh_byte_order("mesh")?;
        reader
            .view
            .seek(view.start() + super::mesh_hdr::LEN)
            .ok_or_else(|| CodecError::malformed("mesh count source is absent"))?;
        Ok((
            order,
            reader.count(order, "mesh point count")?,
            reader.count(order, "mesh facet count")?,
        ))
    })?;
    if point_count != vertices.len() || facet_count != facets.len() {
        return Err(CodecError::malformed("mesh source population differs"));
    }
    let point_start = super::mesh_hdr::LEN + 8;
    let facet_start = point_start
        .checked_add(
            point_count
                .checked_mul(12)
                .ok_or_else(|| CodecError::malformed("mesh point extent overflows"))?,
        )
        .ok_or_else(|| CodecError::malformed("mesh facet start overflows"))?;
    let mut points = alloc_slots(ctx, point_count, None, "projected mesh source point slots")?;
    for (index, output) in points
        .iter_mut()
        .enumerate()
        .filter(|(index, _)| vertices[*index].is_some())
    {
        let extent = child(view, point_start + index * 12, 12)?;
        *output = Some(ctx.record_fragment(owner, extent, || {
            Reader { view: extent }.point3(order, "mesh point")
        })?);
    }
    let mut triangles = alloc_slots(ctx, facet_count, None, "projected mesh source facet slots")?;
    for (index, output) in triangles
        .iter_mut()
        .enumerate()
        .filter(|(index, _)| facets[*index].is_some())
    {
        let extent = child(view, facet_start + index * super::mesh_facet::LEN, 12)?;
        *output = Some(ctx.record_fragment(owner, extent, || {
            let mut reader = Reader { view: extent };
            let points = [
                reader.index(order, point_count, "mesh facet point")?,
                reader.index(order, point_count, "mesh facet point")?,
                reader.index(order, point_count, "mesh facet point")?,
            ];
            Ok(points)
        })?);
    }
    Ok((points, triangles))
}

pub(super) fn point(
    ctx: &DecodeContext<'_>,
    owner: &RecordOwner<'_>,
    view: View<'_>,
    ordinal: usize,
    arity: usize,
) -> Result<FinitePoint3, CodecError> {
    let header = child(view, 0, 4)?;
    let count = ctx.record_fragment(owner, header, || {
        Reader { view: header }.count(ByteOrder::Little, "point-cloud point count")
    })?;
    if count != arity || ordinal >= count {
        return Err(CodecError::malformed(
            "point source ordinal or arity differs",
        ));
    }
    let extent = child(view, 4 + ordinal * 12, 12)?;
    ctx.record_fragment(owner, extent, || {
        Reader { view: extent }.point3(ByteOrder::Little, "point-cloud point")
    })
}

pub(super) fn child(view: View<'_>, offset: usize, length: usize) -> Result<View<'_>, CodecError> {
    let start = view
        .start()
        .checked_add(offset)
        .ok_or_else(|| CodecError::malformed("kernel source offset overflows"))?;
    let end = start
        .checked_add(length)
        .ok_or_else(|| CodecError::malformed("kernel source end overflows"))?;
    view.child(start, end)
        .ok_or_else(|| CodecError::malformed("kernel source getter escapes its member"))
}

pub(super) fn control<T>(
    ctx: &crate::property_reader::PropertyReader<'_, '_>,
    property: super::parts::KernelProperty<'_>,
    application: Option<&crate::native::PropertyRecord>,
    gui: Option<(
        &crate::xml_framing::XmlSource<'_>,
        &crate::fidelity::ValueSourceRanges,
    )>,
    carrier: &crate::native::ValueRecord,
    id: &str,
    produce: impl FnOnce(Option<&RecordOwner<'_>>, crate::native::ValueRecord) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    if let Some(application) = application {
        let mut source = ctx.control_fields(application, carrier.order, id, &["file", "mtrx"])?;
        let mut value = carrier.clone();
        value.attributes = std::mem::take(&mut source.attributes);
        return produce(source.owner(), value);
    }
    let Some((xml, ranges)) = gui else {
        return produce(None, carrier.clone());
    };
    let property_range = ranges.record_range();
    let property_index = xml
        .elements()
        .iter()
        .position(|element| {
            element.extent().start() == xml.view().start() + property_range.start
                && element.extent().end() == xml.view().start() + property_range.end
        })
        .ok_or_else(|| CodecError::malformed("GUI kernel has no actual property source"))?;
    let range = ranges
        .value_ranges()
        .find_map(|(ordinal, range)| (ordinal == carrier.order).then_some(range))
        .ok_or_else(|| CodecError::malformed("GUI kernel has no actual carrier source"))?;
    let value_index = xml
        .elements()
        .iter()
        .position(|element| {
            element.extent().start() == xml.view().start() + range.start
                && element.extent().end() == xml.view().start() + range.end
        })
        .ok_or_else(|| CodecError::malformed("GUI kernel carrier has no original element"))?;
    let (value, owner) = ctx.record_owner(id, xml.extent(property_index)?, || {
        let kind = xml
            .attribute(ctx, property_index, "type")?
            .ok_or_else(|| CodecError::malformed("GUI kernel property type is absent"))?;
        let name = xml
            .attribute(ctx, property_index, "name")?
            .ok_or_else(|| CodecError::malformed("GUI kernel property name is absent"))?;
        if kind != property.type_name
            || name != property.name
            || xml.read_name(value_index)? != carrier.tag
        {
            return Err(CodecError::malformed("GUI kernel property control differs"));
        }
        let mut value = carrier.clone();
        value.attributes.clear();
        for field in ["file", "mtrx"] {
            if let Some(attribute) = xml.attribute(ctx, value_index, field)? {
                value.attributes.insert(field.to_owned(), attribute);
            }
        }
        Ok(value)
    })?;
    produce(Some(&owner), value)
}
