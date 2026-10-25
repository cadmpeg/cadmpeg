// SPDX-License-Identifier: Apache-2.0
//! Independent Appendix C framing for fixed-width application side entries.

use super::{append, level};
use crate::framing::{FrameParent, FramedPart};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use std::ops::Range;

#[cfg(test)]
mod tests;

/// Runtime grammar selected by the actual XML property header.
#[derive(Debug, Clone, Copy)]
pub enum BinaryKind {
    /// Counted packed-u32 colours.
    Color,
    /// Counted little-endian f64 scalars.
    Float,
    /// Counted vector triples; exact member size selects f32 or f64.
    Vector,
    /// Counted seven-component placements with the same precision rule.
    Placement,
    /// Counted edge-id and two-radius records.
    Fillet,
    /// Fixed material fields followed by version-dependent strings.
    Material {
        /// Version from the actual MaterialList XML carrier.
        version: u32,
    },
    /// Runtime-selected one-, three-, or eight-component f32 records.
    F32 {
        /// Number of f32 components selected by the XML runtime type.
        components: usize,
    },
    /// Binding word followed by six counted arrays.
    MeshMaterial,
    /// Mesh header, point and facet counts, fixed rows, and bounds.
    Mesh,
    /// Counted little-endian f32 point triples.
    Points,
}

/// Frame one genuinely opened ZIP member using Appendix C field widths.
/// Invalid framing returns an error after preserving the independently reached
/// frames; semantic scalar values do not affect these boundaries.
pub fn frame_binary_member<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    kind: BinaryKind,
    zip_parent: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let (entry, row, width) = match kind {
        BinaryKind::Color => ("color_list_entry", "color_list_color", 4),
        BinaryKind::Float => ("float_list_entry", "float_list_value", 8),
        BinaryKind::Vector => ("vector_list_entry", "vector_list_vector", 24),
        BinaryKind::Placement => ("placement_list_entry", "placement_list_placement", 56),
        BinaryKind::Fillet => ("fillet_edges_entry", "fillet_edge", 20),
        BinaryKind::Material { .. } => ("material_list_entry", "material_list_material", 24),
        BinaryKind::F32 {
            components: 1 | 3 | 8,
        } => ("f32_record_list_entry", "f32_record", 0),
        BinaryKind::F32 { .. } => {
            return Err("f32 record component width is not in Appendix C".into());
        }
        BinaryKind::MeshMaterial => ("mesh_material_entry", "", 0),
        BinaryKind::Mesh => ("mesh_kernel_entry", "", 0),
        BinaryKind::Points => ("point_kernel_entry", "point_kernel_point", 12),
    };
    let len = source.window().len();
    let mut header = 4;
    if matches!(kind, BinaryKind::Material { version: 0 | 1 })
        && View::u32_le_at(source.window(), 0).is_some_and(|word| word & 0x8000_0000 != 0)
    {
        header = 8;
    }
    let framing = match kind {
        BinaryKind::MeshMaterial => vec![],
        BinaryKind::Mesh => [(0, 8), (264, 8)]
            .into_iter()
            .filter(|(start, size)| start + size <= len)
            .map(|(start, size)| start..start + size)
            .collect(),
        _ => {
            if header <= len {
                vec![0..header]
            } else {
                vec![]
            }
        }
    };
    let parent = push(ctx, source, entry, zip_parent, 0..len, framing, out)?;
    match kind {
        BinaryKind::Mesh => return mesh(ctx, source, parent, out),
        BinaryKind::MeshMaterial => return mesh_material(ctx, source, parent, out),
        _ => {}
    }
    let count = word(source, header - 4)?;
    let count = usize::try_from(count).map_err(|_| "binary count exceeds usize")?;
    if let BinaryKind::Material { version } = kind {
        return materials(ctx, source, parent, header, count, version, out);
    }
    let width = match kind {
        BinaryKind::F32 { components } => components.checked_mul(4).ok_or("f32 width overflows")?,
        BinaryKind::Vector | BinaryKind::Placement
            if count
                .checked_mul(width / 2)
                .and_then(|bytes| bytes.checked_add(header))
                == Some(len) =>
        {
            width / 2
        }
        _ => width,
    };
    let mut at = header;
    for _ in 0..count {
        let end = at.checked_add(width).ok_or("binary row extent overflows")?;
        push(ctx, source, row, parent, at..end.min(len), vec![], out)?;
        if end > len {
            return Err("binary row is cut short".into());
        }
        at = end;
    }
    if at != len {
        return Err("binary list has trailing bytes".into());
    }
    Ok(())
}

fn mesh<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    parent: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let len = source.window().len();
    push(
        ctx,
        source,
        "mesh_kernel_header",
        parent,
        0..len.min(264),
        if len >= 8 { vec![0..8] } else { vec![] },
        out,
    )?;
    if len < 272 {
        return Err("mesh count header is cut short".into());
    }
    let little = View::u32_le_at(source.window(), 0) == Some(0xa0b0c0d0)
        && View::u32_le_at(source.window(), 4) == Some(0x00010000);
    let big = View::u32_le_at(source.window(), 0).map(u32::swap_bytes) == Some(0xa0b0c0d0)
        && View::u32_le_at(source.window(), 4).map(u32::swap_bytes) == Some(0x00010000);
    if !little && !big {
        return Err("mesh byte order is not located".into());
    }
    let read = |at| {
        if little {
            View::u32_le_at(source.window(), at)
        } else {
            View::u32_le_at(source.window(), at).map(u32::swap_bytes)
        }
    };
    let points = usize::try_from(read(264).ok_or("missing mesh point count")?)
        .map_err(|_| "mesh count exceeds usize")?;
    let facets = usize::try_from(read(268).ok_or("missing mesh facet count")?)
        .map_err(|_| "mesh count exceeds usize")?;
    let facet_start = points
        .checked_mul(12)
        .and_then(|bytes| 272_usize.checked_add(bytes))
        .ok_or("mesh point extent overflows")?;
    let bounds = facets
        .checked_mul(24)
        .and_then(|bytes| facet_start.checked_add(bytes))
        .ok_or("mesh facet extent overflows")?;
    if bounds.checked_add(24).is_some_and(|end| end < len) {
        return Err("mesh bounds do not end at member boundary".into());
    }
    for index in 0..points {
        let start = 272 + index * 12;
        if start > len {
            break;
        }
        push(
            ctx,
            source,
            "mesh_point",
            parent,
            start..(start + 12).min(len),
            vec![],
            out,
        )?;
        if start + 12 > len {
            break;
        }
    }
    for index in 0..facets {
        let start = facet_start + index * 24;
        if start > len {
            break;
        }
        push(
            ctx,
            source,
            "mesh_facet",
            parent,
            start..(start + 24).min(len),
            vec![],
            out,
        )?;
        if start + 24 > len {
            break;
        }
    }
    if bounds <= len {
        push(
            ctx,
            source,
            "mesh_bounding_box",
            parent,
            bounds..len,
            vec![],
            out,
        )?;
    }
    Ok(())
}

fn mesh_material<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    parent: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let len = source.window().len();
    push(
        ctx,
        source,
        "mesh_material_binding",
        parent,
        0..len.min(4),
        vec![],
        out,
    )?;
    if len < 4 {
        return Err("mesh material binding is cut short".into());
    }
    let mut at = 4usize;
    for _ in 0..6 {
        let count = match word(source, at) {
            Ok(count) => usize::try_from(count).map_err(|_| "mesh material count exceeds usize")?,
            Err(error) => {
                push(
                    ctx,
                    source,
                    "mesh_material_array",
                    parent,
                    at..len,
                    vec![],
                    out,
                )?;
                return Err(error);
            }
        };
        let values = at
            .checked_add(4)
            .ok_or("mesh array values start overflows")?;
        let end = count
            .checked_mul(4)
            .and_then(|size| values.checked_add(size))
            .ok_or("mesh array extent overflows")?;
        let array = push(
            ctx,
            source,
            "mesh_material_array",
            parent,
            at..end.min(len),
            vec![at..values],
            out,
        )?;
        let mut value = values;
        for _ in 0..count {
            let stop = value
                .checked_add(4)
                .ok_or("mesh material value extent overflows")?;
            push(
                ctx,
                source,
                "mesh_material_value",
                array,
                value..stop.min(len),
                vec![],
                out,
            )?;
            if stop > len {
                return Err("mesh material value is cut short".into());
            }
            value = stop;
        }
        at = end;
    }
    if at != len {
        return Err("mesh material has trailing bytes".into());
    }
    Ok(())
}
fn materials<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    parent: usize,
    mut at: usize,
    count: usize,
    version: u32,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    if version > 3 {
        return Err("unsupported material version".into());
    }
    let len = source.window().len();
    for _ in 0..count {
        let end = at
            .checked_add(24)
            .ok_or("material fixed extent overflows")?;
        push(
            ctx,
            source,
            "material_list_material",
            parent,
            at..end.min(len),
            vec![],
            out,
        )?;
        if end > len {
            return Err("material fixed record is cut short".into());
        }
        at = end;
    }
    if version == 3 {
        let strings = count
            .checked_mul(3)
            .ok_or("material string population overflows")?;
        for _ in 0..strings {
            let count = match word(source, at) {
                Ok(count) => {
                    usize::try_from(count).map_err(|_| "material string count exceeds usize")?
                }
                Err(error) => {
                    push(
                        ctx,
                        source,
                        "material_list_string",
                        parent,
                        at..len,
                        vec![],
                        out,
                    )?;
                    return Err(error);
                }
            };
            let payload = at.checked_add(4).ok_or("material string start overflows")?;
            let end = payload
                .checked_add(count)
                .ok_or("material string extent overflows")?;
            push(
                ctx,
                source,
                "material_list_string",
                parent,
                at..end.min(len),
                vec![at..payload],
                out,
            )?;
            if end > len {
                return Err("material string is cut short".into());
            }
            at = end;
        }
    }
    if at != len {
        return Err("material list has trailing bytes".into());
    }
    Ok(())
}

fn word(source: View<'_>, at: usize) -> Result<u32, String> {
    View::u32_le_at(source.window(), at).ok_or_else(|| "binary count field is cut short".into())
}
fn push<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    name: &str,
    parent: usize,
    extent: Range<usize>,
    framing: Vec<Range<usize>>,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<usize, String> {
    let from = source
        .start()
        .checked_add(extent.start)
        .ok_or("binary frame start overflows")?;
    let to = source
        .start()
        .checked_add(extent.end)
        .ok_or("binary frame end overflows")?;
    let view = source
        .child(from, to)
        .ok_or("binary frame exceeds actual member")?;
    let framing = framing
        .into_iter()
        .map(|range| {
            let from = source
                .start()
                .checked_add(range.start)
                .ok_or("binary framing start overflows")?;
            let to = source
                .start()
                .checked_add(range.end)
                .ok_or("binary framing end overflows")?;
            Ok(u64_from_index(from)..u64_from_index(to))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let index = out.len();
    let part = FramedPart::new(
        ctx,
        level(&format!("freecad.{name}"))?,
        view,
        FrameParent::Part(parent),
        framing,
        vec![],
    )?;
    append(ctx, out, part)?;
    Ok(index)
}
