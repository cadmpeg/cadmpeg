// SPDX-License-Identifier: Apache-2.0
//! Direct Catmull-Clark control cages (§19.5).

use super::WriteError;
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
        if version.value() < 60 {
            match write_proxy(
                ctx, ir, version, session, output, materials, surface, losses,
            ) {
                Ok(()) => count += 1,
                Err(WriteError::Omit(reason)) => losses.push(
                    RhinoLossCode::WriterSubdOmitted
                        .note(format!("SubD {} omitted: {reason}", surface.id)),
                ),
                Err(WriteError::Fail(error)) => return Err(error),
            }
            continue;
        }
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

/// The archive-50 control-net proxy (`docs/formats/rhino_3dm.md` §7.2.6):
/// a top-level `ON_Mesh` carrying the cage, with a proxy userdata item
/// holding the embedded SubD. A modern reader promotes the mesh back to
/// the exact SubD; an archive-50 reader keeps the control net.
fn write_proxy(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    version: RhinoArchiveVersion,
    session: &EncodeSession,
    output: &mut EncodeBuffer<'_>,
    materials: &super::MaterialPlan,
    surface: &SubdSurface,
    _losses: &mut Vec<LossNote>,
) -> Result<(), WriteError> {
    if let Some(reason) = omission(surface, version) {
        return Err(WriteError::Omit(reason.into()));
    }
    let (points, triangles) = control_net(surface)?;
    let quads = triangles
        .iter()
        .map(|triangle| [triangle[0], triangle[1], triangle[2], triangle[2]])
        .collect::<Vec<_>>();
    let vertices = points
        .iter()
        .map(|point| {
            [point.x, point.y, point.z]
                .into_iter()
                .map(|value| {
                    cadmpeg_core::convert::f32_from_f64(value)
                        .and_then(cadmpeg_ir::scalar::FiniteBinary32::new)
                        .ok_or_else(|| {
                            WriteError::Omit("control-net point exceeds f32 range".into())
                        })
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let vertices = vertices
        .into_iter()
        .map(|row| [row[0], row[1], row[2]])
        .collect::<Vec<_>>();
    let fingerprint = crate::mesh::native_proxy_fingerprint(&quads, &vertices);
    let mesh = cadmpeg_ir::tessellation::Tessellation::new(
        cadmpeg_ir::tessellation::TessellationId::mint(format!(
            "cadir:model:tessellation#rhino-proxy-{}",
            surface
                .id
                .as_str()
                .rsplit('#')
                .next_back()
                .unwrap_or("subd")
        ))
        .map_err(|error| WriteError::Fail(CodecError::malformed(error.to_string())))?,
        cadmpeg_ir::tessellation::TessellationMesh::from_list_lanes(points, triangles, None)
            .map_err(|error| WriteError::Omit(error.to_string()))?,
        Vec::new(),
    )
    .map_err(|error| WriteError::Fail(CodecError::malformed(error.to_string())))?;
    let mesh_payload = super::mesh_payload(&mesh, version)?;
    let Some(embedded) = payload(ctx, session, surface, version)? else {
        return Err(WriteError::Omit("a face repeats a control edge".into()));
    };
    let item = proxy_item(&fingerprint, embedded.as_bytes(), version)?;
    emit_proxy_object(ctx, ir, &mesh_payload, &item, surface, materials, output)?;
    Ok(())
}

fn emit_proxy_object(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    payload: &super::MeshPayload,
    item: &[u8],
    surface: &SubdSurface,
    materials: &super::MaterialPlan,
    output: &mut EncodeBuffer<'_>,
) -> Result<(), WriteError> {
    let display = super::presentation::display(
        ir,
        &cadmpeg_ir::appearance::AppearanceTarget::Record {
            arena: SUBDS,
            id: surface.id.to_string(),
        },
        surface.source_object.as_ref(),
    );
    ctx.charge_entities(1, "write Rhino object record")?;
    output.append(
        &mesh_object_with_proxy(ir, payload, item, surface.id.as_str(), display, materials)?,
        "retain Rhino archive bytes",
    )?;
    Ok(())
}

/// The cage control net as triangle soup: vertices in cage order with one
/// fan per face. The proxy fingerprint hashes this same layout.
fn control_net(
    surface: &SubdSurface,
) -> Result<(Vec<cadmpeg_ir::math::Point3>, Vec<[u32; 3]>), WriteError> {
    use cadmpeg_core::decode::index_from_u32;
    let points = surface
        .cage
        .vertices()
        .iter()
        .map(|vertex| vertex.point().get())
        .collect::<Vec<_>>();
    let mut triangles = Vec::new();
    for face in surface.cage.faces() {
        let mut ring = Vec::new();
        for use_ in face.edges() {
            let edge = &surface.cage.edges()[index_from_u32(use_.edge)];
            let ends = edge.vertices();
            ring.push(if use_.reversed { ends[1] } else { ends[0] });
        }
        if ring.len() < 3 {
            return Err(WriteError::Omit(
                "control-net face has fewer than three vertices".into(),
            ));
        }
        for corner in &ring {
            if usize::try_from(*corner).map_err(|_| {
                WriteError::Fail(CodecError::malformed(
                    "control-net vertex index exceeds address space",
                ))
            })? >= points.len()
            {
                return Err(WriteError::Omit(
                    "control-net face names a missing vertex".into(),
                ));
            }
        }
        for window in ring.windows(2).skip(1) {
            triangles.push([ring[0], window[0], window[1]]);
        }
    }
    Ok((points, triangles))
}

/// The `ON_SubDMeshProxyUserData` item: identity transform, embedded SubD,
/// parent counts, and parent-array hashes, per the proxy contract.
fn proxy_item(
    fingerprint: &crate::subd::MeshProxyFingerprint,
    embedded: &[u8],
    version: RhinoArchiveVersion,
) -> Result<Vec<u8>, WriteError> {
    use crate::subd::SUBD_MESH_PROXY_USERDATA;
    let mut transform = Vec::new();
    for index in 0..16 {
        transform.extend((if index % 5 == 0 { 1.0_f64 } else { 0.0_f64 }).to_le_bytes());
    }
    let mut body = Vec::new();
    body.extend(1_i32.to_le_bytes());
    body.extend(1_i32.to_le_bytes());
    body.push(1);
    body.extend_from_slice(embedded);
    body.extend(
        i32::try_from(fingerprint.face_count)
            .map_err(|_| {
                WriteError::Fail(super::native_count_limit(
                    2_147_483_647,
                    cadmpeg_core::decode::u64_from_index(fingerprint.face_count),
                ))
            })?
            .to_le_bytes(),
    );
    body.extend(
        i32::try_from(fingerprint.vertex_count)
            .map_err(|_| {
                WriteError::Fail(super::native_count_limit(
                    2_147_483_647,
                    cadmpeg_core::decode::u64_from_index(fingerprint.vertex_count),
                ))
            })?
            .to_le_bytes(),
    );
    for digest in [fingerprint.face_sha1, fingerprint.vertex_sha1] {
        let mut hash = Vec::new();
        hash.extend(1_i32.to_le_bytes());
        hash.extend(0_i32.to_le_bytes());
        hash.extend_from_slice(&digest);
        body.extend(super::crc_chunk(ANONYMOUS, &hash)?);
    }
    let payload = super::crc_chunk(ANONYMOUS, &body)?;
    let mut header = Vec::new();
    header.extend(crate::wire::Uuid::to_wire(SUBD_MESH_PROXY_USERDATA));
    header.extend(crate::wire::Uuid::to_wire(SUBD_MESH_PROXY_USERDATA));
    header.extend(1_i32.to_le_bytes());
    header.extend_from_slice(&transform);
    header.extend(PROXY_APPLICATION_UUID);
    header.push(0);
    header.extend(
        i32::try_from(version.value())
            .map_err(|_| {
                WriteError::Fail(super::native_count_limit(2_147_483_647, version.value()))
            })?
            .to_le_bytes(),
    );
    header.extend(WRITER_VERSION.to_le_bytes());
    let header = super::crc_chunk(crate::objects::CLASS_USERDATA_HEADER, &header)?;
    let mut item = vec![PROXY_ITEM_VERSION];
    item.extend(header);
    item.extend(payload);
    Ok(super::long_chunk(crate::objects::CLASS_USERDATA, &item)?)
}

/// The application UUID the specification assigns to proxy userdata
/// (`ON_opennurbs6_id`, `docs/formats/rhino_3dm.md` §7.2.6).
const PROXY_APPLICATION_UUID: [u8; 16] = [
    0x7b, 0x0b, 0x58, 0x5d, 0x7a, 0x31, 0x45, 0xd0, 0x92, 0x5e, 0xbd, 0xd7, 0xdd, 0xf3, 0xe4, 0xe3,
];

/// The writer stamp carried by proxy save contexts.
const WRITER_VERSION: i32 = 202_401_010;

/// The proxy item version: class-userdata 2.2.
const PROXY_ITEM_VERSION: u8 = 0x22;

/// A mesh object record carrying the SubD proxy userdata item.
fn mesh_object_with_proxy(
    ir: &CadIr,
    payload: &super::MeshPayload,
    item: &[u8],
    identity: &str,
    display: super::presentation::Display<'_>,
    materials: &super::MaterialPlan,
) -> Result<Vec<u8>, WriteError> {
    let object_type = super::short_chunk(super::TCODE_OBJECT_RECORD_TYPE, 0x20);
    let mut uuid_body = super::MESH_CLASS.to_vec();
    uuid_body.extend(crc32fast::hash(&super::MESH_CLASS).to_le_bytes());
    let uuid = super::long_chunk(super::TCODE_CLASS_UUID, &uuid_body)?;
    let class_data =
        super::crc_chunk_with_direct(super::TCODE_CLASS_DATA, &payload.body, &payload.direct)?;
    let class_end = super::short_chunk(super::TCODE_CLASS_END, 0);
    let class = super::long_chunk(
        super::TCODE_CLASS_WRAPPER,
        &[uuid, class_data, item.to_vec(), class_end].concat(),
    )?;
    let object_end = super::short_chunk(super::TCODE_OBJECT_RECORD_END, 0);
    let mut body = [object_type, class].concat();
    body.extend(super::crc_chunk(
        super::TCODE_OBJECT_RECORD_ATTRIBUTES,
        &super::object_attributes_payload(
            ir,
            identity,
            display.name,
            display.color,
            display.visible,
            materials,
        )?,
    )?);
    body.extend(object_end);
    Ok(super::zero_crc_chunk(super::TCODE_OBJECT_RECORD, &body)?)
}

fn omission(surface: &SubdSurface, version: RhinoArchiveVersion) -> Option<&'static str> {
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
    // Working scratch, not retained output: it never reaches the archive.
    let mut result = Vec::new();
    ctx.reserve_working(&mut result, count, "Rhino SubD incidence")?;
    let mut values = values;
    let mut staged = 0;
    while staged < count {
        let Some(value) = values.next() else {
            break;
        };
        result.push(value);
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
