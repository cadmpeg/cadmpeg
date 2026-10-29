// SPDX-License-Identifier: Apache-2.0
//! Direct Catmull-Clark control cages (§19.5).

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{EncodeBuffer, EncodeSession};
use cadmpeg_ir::document::ArenaName;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::subd::{SubdEdgeTag, SubdSurface, SubdVertexTag};
use cadmpeg_ir::CadIr;

use crate::loss::RhinoLossCode;
use crate::RhinoArchiveVersion;

const ANONYMOUS: u32 = 0x4000_8000;
const SUBD_OBJECT: i64 = 0x0004_0000;
const OPERATION: &str = "retain Rhino SubD bytes";

/// The registered neutral arena that holds `SubD` records.
const SUBDS: ArenaName = {
    let name = b"subds";
    let mut index = 0;
    loop {
        assert!(
            index < ArenaName::ALL.len(),
            "subds is a registered neutral arena"
        );
        let candidate = ArenaName::ALL[index].as_str().as_bytes();
        if candidate.len() == name.len() {
            let mut at = 0;
            while at < name.len() && candidate[at] == name[at] {
                at += 1;
            }
            if at == name.len() {
                break ArenaName::ALL[index];
            }
        }
        index += 1;
    }
};

pub(super) fn write(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    version: RhinoArchiveVersion,
    session: &EncodeSession,
    output: &mut EncodeBuffer<'_>,
    materials: &super::MaterialPlan,
    losses: &mut Vec<LossNote>,
) -> Result<usize, CodecError> {
    let mut count = 0;
    for surface in &ir.model.subds {
        if let Some(reason) = omission(surface, version) {
            losses.push(
                RhinoLossCode::WriterSubdOmitted
                    .note(format!("SubD {} omitted: {reason}", surface.id)),
            );
            continue;
        }
        let Some(payload) = payload(ctx, session, surface, version)? else {
            losses.push(RhinoLossCode::WriterSubdOmitted.note(format!(
                "SubD {} omitted: a face repeats a control edge",
                surface.id
            )));
            continue;
        };
        let display = super::presentation::display(
            ir,
            &cadmpeg_ir::appearance::AppearanceTarget::Record {
                arena: SUBDS,
                id: surface.id.to_string(),
            },
            surface.source_object.as_ref(),
        );
        let attributes = super::object_attributes_payload(
            ir,
            surface.id.as_str(),
            display.name,
            display.color,
            display.visible,
            materials,
        )?;
        ctx.charge_entities(1, "write Rhino object record")?;
        output.append(
            &super::framed_object_record(
                SUBD_OBJECT,
                crate::subd::ON_SUBD.to_wire(),
                payload.as_bytes(),
                Some(&[1]),
                Some(attributes),
            )?,
            "retain Rhino archive bytes",
        )?;
        count += 1;
    }
    Ok(count)
}

fn omission(surface: &SubdSurface, version: RhinoArchiveVersion) -> Option<&'static str> {
    if version.value() < 60 {
        return Some("archives below 60 require a SubD mesh proxy, which is not implemented");
    }
    if !surface.cage.symmetries().is_empty()
        || surface
            .cage
            .vertices()
            .iter()
            .any(|vertex| vertex.secondary_grips().is_some())
        || surface
            .cage
            .edges()
            .iter()
            .any(|edge| edge.knot_interval().is_some())
    {
        return Some(
            "editor symmetry, secondary grips or knot intervals have no direct cage encoding",
        );
    }
    if version.value() < 80
        && surface.cage.edges().iter().any(|edge| {
            let [start, end] = edge.sharpness();
            start != end
        })
    {
        return Some("distinct endpoint sharpness requires archive 80");
    }
    None
}

#[derive(Clone, Copy, Default)]
struct Pointer {
    id: u32,
    flags: u8,
}

#[derive(Clone, Copy, Default)]
struct Incidence {
    owner: u32,
    pointer: Pointer,
}

fn incidence(
    ctx: &DecodeContext<'_>,
    count: usize,
    values: impl Iterator<Item = Incidence>,
) -> Result<Vec<Incidence>, CodecError> {
    let mut result = ctx.alloc_filled(count, Incidence::default(), "Rhino SubD incidence")?;
    let mut values = values;
    let mut staged = 0;
    for (slot, value) in result.iter_mut().zip(values.by_ref()) {
        *slot = value;
        staged += 1;
    }
    debug_assert!(
        staged == count && values.next().is_none(),
        "the incidence count equals the admitted edge-use count"
    );
    result.truncate(staged);
    result.sort_unstable_by_key(|entry| (entry.owner, entry.pointer.id));
    Ok(result)
}

fn owned(entries: &[Incidence], owner: u32) -> &[Incidence] {
    let start = entries.partition_point(|entry| entry.owner < owner);
    let end = entries.partition_point(|entry| entry.owner <= owner);
    &entries[start..end]
}

fn count32(value: u64) -> Result<u32, CodecError> {
    u32::try_from(value).map_err(|_| super::native_count_limit(u64::from(u32::MAX), value))
}

fn count16(value: usize) -> Result<u16, CodecError> {
    u16::try_from(value)
        .map_err(|_| super::native_count_limit(u64::from(u16::MAX), u64_from_index(value)))
}

fn payload<'a>(
    ctx: &DecodeContext<'_>,
    session: &'a EncodeSession,
    surface: &SubdSurface,
    version: RhinoArchiveVersion,
) -> Result<Option<EncodeBuffer<'a>>, CodecError> {
    let vertices = surface.cage.vertices();
    let edges = surface.cage.edges();
    let faces = surface.cage.faces();
    let p1 = count32(1 + u64_from_index(vertices.len()))?;
    let p2 = count32(u64::from(p1) + u64_from_index(edges.len()))?;
    let p3 = count32(u64::from(p2) + u64_from_index(faces.len()))?;
    // Each face count is stored twice as u16, before its directed pointers.
    let mut uses = 0_u64;
    for face in faces {
        uses += u64::from(count16(face.edges().len())?);
    }
    let uses = usize::try_from(uses)
        .map_err(|_| super::native_count_limit(u64_from_index(usize::MAX), uses))?;
    let edge_use_count = u64_from_index(edges.len()) * 2;
    let edge_use_count = usize::try_from(edge_use_count)
        .map_err(|_| super::native_count_limit(u64_from_index(usize::MAX), edge_use_count))?;
    let vertex_edges = incidence(
        ctx,
        edge_use_count,
        edges.iter().zip(p1..p2).flat_map(|(edge, id)| {
            edge.vertices()
                .into_iter()
                .enumerate()
                .map(move |(end, owner)| Incidence {
                    owner,
                    pointer: Pointer {
                        id,
                        flags: u8::from(end == 1),
                    },
                })
        }),
    )?;
    let edge_faces = incidence(
        ctx,
        uses,
        faces.iter().zip(p2..p3).flat_map(|(face, id)| {
            face.edges().iter().map(move |use_| Incidence {
                owner: use_.edge,
                pointer: Pointer {
                    id,
                    flags: u8::from(use_.reversed),
                },
            })
        }),
    )?;
    if edge_faces
        .windows(2)
        .any(|pair| pair[0].owner == pair[1].owner && pair[0].pointer.id == pair[1].pointer.id)
    {
        return Ok(None);
    }
    let mut vertex_faces = incidence(
        ctx,
        uses,
        faces.iter().zip(p2..p3).flat_map(|(face, id)| {
            face.edges().iter().map(move |use_| {
                let edge = &edges[cadmpeg_core::decode::index_from_u32(use_.edge)];
                let ends = edge.vertices();
                Incidence {
                    owner: ends[usize::from(use_.reversed)],
                    pointer: Pointer { id, flags: 0 },
                }
            })
        }),
    )?;
    vertex_faces.dedup_by_key(|entry| (entry.owner, entry.pointer.id));

    let mut bounds = [0.0_f64; 6];
    if let Some(first) = vertices.first() {
        let point = first.point().get();
        bounds = [point.x, point.y, point.z, point.x, point.y, point.z];
        for vertex in &vertices[1..] {
            let point = vertex.point().get();
            for (axis, value) in [point.x, point.y, point.z].into_iter().enumerate() {
                bounds[axis] = bounds[axis].min(value);
                bounds[axis + 3] = bounds[axis + 3].max(value);
            }
        }
    }
    let mut output = session.buffer();
    output.append(&[1], OPERATION)?;
    let subdimple = begin_chunk(&mut output)?;
    output.append(&1_i32.to_le_bytes(), OPERATION)?;
    output.append(&0_i32.to_le_bytes(), OPERATION)?;
    for value in [1, p1 - 1, p2 - p1, p3 - p2] {
        output.append(&value.to_le_bytes(), OPERATION)?;
    }
    write_bounds(&mut output, bounds)?;
    let level = begin_chunk(&mut output)?;
    output.append(&1_i32.to_le_bytes(), OPERATION)?;
    output.append(&1_i32.to_le_bytes(), OPERATION)?;
    output.append(&0_u16.to_le_bytes(), OPERATION)?;
    output.append(&[4, 4, 4], OPERATION)?;
    write_bounds(&mut output, bounds)?;
    for partition in [1, p1, p2, p3] {
        output.append(&partition.to_le_bytes(), OPERATION)?;
    }
    for (vertex, id) in vertices.iter().zip(1..p1) {
        let index = id - 1;
        base(&mut output, id, id, version)?;
        output.append(
            &[match vertex.tag {
                SubdVertexTag::Smooth => 1,
                SubdVertexTag::Crease => 2,
                SubdVertexTag::Corner => 3,
                SubdVertexTag::Dart => 4,
            }],
            OPERATION,
        )?;
        let point = vertex.point().get();
        for value in [point.x, point.y, point.z] {
            output.append(&value.to_le_bytes(), OPERATION)?;
        }
        let incident_edges = owned(&vertex_edges, index);
        let incident_faces = owned(&vertex_faces, index);
        output.append(&count16(incident_edges.len())?.to_le_bytes(), OPERATION)?;
        output.append(&count16(incident_faces.len())?.to_le_bytes(), OPERATION)?;
        output.append(&[0], OPERATION)?;
        pointers(&mut output, incident_edges)?;
        pointers(&mut output, incident_faces)?;
        record_end(&mut output, version)?;
    }
    for ((edge, id), ordinal) in edges.iter().zip(p1..p2).zip(1..=p2 - p1) {
        let index = ordinal - 1;
        base(&mut output, id, ordinal, version)?;
        output.append(
            &[match edge.tag {
                SubdEdgeTag::Smooth => 1,
                SubdEdgeTag::Crease => 2,
                SubdEdgeTag::SmoothX => 4,
            }],
            OPERATION,
        )?;
        let incident_faces = owned(&edge_faces, index);
        output.append(&count16(incident_faces.len())?.to_le_bytes(), OPERATION)?;
        for value in edge.sector_coefficients() {
            output.append(&value.get().to_le_bytes(), OPERATION)?;
        }
        let [start, end] = edge.sharpness();
        output.append(&start.get().to_le_bytes(), OPERATION)?;
        output.append(&2_u16.to_le_bytes(), OPERATION)?;
        for vertex in edge.vertices() {
            pointer(
                &mut output,
                Pointer {
                    id: 1 + vertex,
                    flags: 0,
                },
            )?;
        }
        pointers(&mut output, incident_faces)?;
        if version.value() >= 80 {
            output.append(&[8], OPERATION)?;
            output.append(&end.get().to_le_bytes(), OPERATION)?;
        }
        record_end(&mut output, version)?;
    }
    for ((face, id), ordinal) in faces.iter().zip(p2..p3).zip(1..=p3 - p2) {
        base(&mut output, id, ordinal, version)?;
        output.append(&ordinal.to_le_bytes(), OPERATION)?;
        output.append(&0_u32.to_le_bytes(), OPERATION)?;
        let count = count16(face.edges().len())?;
        output.append(&count.to_le_bytes(), OPERATION)?;
        output.append(&count.to_le_bytes(), OPERATION)?;
        for use_ in face.edges() {
            pointer(
                &mut output,
                Pointer {
                    id: p1 + use_.edge,
                    flags: u8::from(use_.reversed),
                },
            )?;
        }
        record_end(&mut output, version)?;
    }
    output.append(&[0], OPERATION)?;
    end_chunk(&mut output, level, None)?;
    let level_end = output.len();
    end_chunk(&mut output, subdimple, Some(level..level_end))?;
    Ok(Some(output))
}

fn pointer(output: &mut EncodeBuffer<'_>, pointer: Pointer) -> Result<(), CodecError> {
    output.append(&pointer.id.to_le_bytes(), OPERATION)?;
    output.append(&[pointer.flags], OPERATION)?;
    Ok(())
}

fn pointers(output: &mut EncodeBuffer<'_>, entries: &[Incidence]) -> Result<(), CodecError> {
    output.append(&count16(entries.len())?.to_le_bytes(), OPERATION)?;
    for entry in entries {
        pointer(output, entry.pointer)?;
    }
    Ok(())
}

fn base(
    output: &mut EncodeBuffer<'_>,
    archive_id: u32,
    component_id: u32,
    version: RhinoArchiveVersion,
) -> Result<(), CodecError> {
    output.append(&archive_id.to_le_bytes(), OPERATION)?;
    output.append(&component_id.to_le_bytes(), OPERATION)?;
    output.append(&0_u16.to_le_bytes(), OPERATION)?;
    output.append(
        if version.value() < 70 {
            &[0, 0]
        } else {
            &[255]
        },
        OPERATION,
    )?;
    Ok(())
}

fn record_end(
    output: &mut EncodeBuffer<'_>,
    version: RhinoArchiveVersion,
) -> Result<(), CodecError> {
    output.append(&[if version.value() < 70 { 0 } else { 255 }], OPERATION)?;
    Ok(())
}

fn write_bounds(output: &mut EncodeBuffer<'_>, bounds: [f64; 6]) -> Result<(), CodecError> {
    for value in bounds {
        output.append(&value.to_le_bytes(), OPERATION)?;
    }
    Ok(())
}

fn begin_chunk(output: &mut EncodeBuffer<'_>) -> Result<usize, CodecError> {
    let start = output.len();
    output.append(&ANONYMOUS.to_le_bytes(), OPERATION)?;
    output.append(&0_i64.to_le_bytes(), OPERATION)?;
    Ok(start)
}

fn end_chunk(
    output: &mut EncodeBuffer<'_>,
    start: usize,
    child: Option<std::ops::Range<usize>>,
) -> Result<(), CodecError> {
    let mut crc = crc32fast::Hasher::new();
    let body_start = start + 12;
    if let Some(child) = child {
        crc.update(&output.as_bytes()[body_start..child.start]);
        crc.update(&output.as_bytes()[child.end..]);
    } else {
        crc.update(&output.as_bytes()[body_start..]);
    }
    output.append(&crc.finalize().to_le_bytes(), OPERATION)?;
    let length = super::archive_body_len(u64_from_index(output.len() - body_start))?;
    let patched = output.overwrite(start + 4, &length.to_le_bytes());
    debug_assert!(patched, "the staged SubD chunk header exists");
    Ok(())
}

#[cfg(test)]
mod tests;
