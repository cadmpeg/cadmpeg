// SPDX-License-Identifier: Apache-2.0
//! The surface tessellation stream (§6): patch headers, vertex blocks, and
//! triangle primitives.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{index_from_u32, View};
use cadmpeg_core::CodecError;

use super::attributes::AppearanceTable;
use super::tlv::{self, Record};
use super::MemberResidue;
use crate::layout::{patch_header_head as patch_header, token, vertex_block_head as vertex_block};
use crate::read::{at, f32x3, record};

/// Patch-header appearance value for "no appearance" (§5.3).
const NO_APPEARANCE: u32 = 0xffff_ffff;
/// Vertex layout bit 0: a normal follows the position.
const LAYOUT_NORMAL: u8 = 0x01;
/// Vertex layout bit 2: a texture coordinate follows.
const LAYOUT_TEXTURE: u8 = 0x04;
/// Bytes of a position, and of a normal.
const VECTOR_LEN: usize = 12;
/// Bytes of a texture coordinate.
const TEXTURE_LEN: usize = 8;
/// Bytes of one primitive vertex index.
const INDEX_LEN: usize = 4;
/// Tag of the edge header that opens a line group (§6.1, §7.2); the
/// singular-use record of the exact stream (§8.13) has the same tag.
const LINE_GROUP_HEADER: u32 = token::EXACT_SINGULAR_USE;

/// One triangle mesh: the patches of a member that share an appearance and
/// a vertex layout.
#[derive(Debug, Default)]
pub(crate) struct Mesh {
    /// The `base_index` of the shared appearance, if any.
    pub(crate) appearance: Option<u32>,
    /// Positions in stored metres.
    pub(crate) positions: Vec<[f32; 3]>,
    /// One normal per position, when the vertex layout carries normals.
    pub(crate) normals: Option<Vec<[f32; 3]>>,
    pub(crate) triangles: Vec<[u32; 3]>,
    /// Triangle ordinals of each patch, by patch ordinal in the stream.
    pub(crate) patches: Vec<(usize, Vec<u32>)>,
}

/// The vertex block that triangle primitives currently index.
struct Block {
    mesh: usize,
    base: u32,
    count: u32,
}

/// The patch whose records are being read.
struct Patch {
    ordinal: usize,
    appearance: Option<u32>,
    block: Option<Block>,
}

/// Reads the surface tessellation stream.
pub(crate) fn read(
    mut stream: View<'_>,
    stream_id: u32,
    appearances: &AppearanceTable,
    residue: &mut MemberResidue,
) -> Result<Vec<Mesh>, CodecError> {
    tlv::stream_index(&mut stream, stream_id)?;
    let mut meshes: Vec<Mesh> = Vec::new();
    let mut groups: BTreeMap<(Option<u32>, bool), usize> = BTreeMap::new();
    let mut patch: Option<Patch> = None;
    let mut line_group = false;
    let mut patches = 0_usize;
    while let Some(record) = tlv::next(&mut stream)? {
        match record.tag {
            token::TESS_PATCH_HEADER => {
                patch = Some(Patch {
                    ordinal: patches,
                    appearance: patch_appearance(record, appearances, residue)?,
                    block: None,
                });
                patches += 1;
            }
            LINE_GROUP_HEADER => {
                patch = None;
                line_group = true;
                residue.surface_records.count(record.tag);
            }
            token::TESS_VERTEX_BLOCK => {
                let Some(patch) = patch.as_mut() else {
                    if line_group {
                        residue.surface_records.count(record.tag);
                        continue;
                    }
                    return Err(CodecError::malformed(format_args!(
                        "vertex block at offset {} precedes every patch and line group header",
                        record.payload.start()
                    )));
                };
                patch.block = Some(read_vertices(
                    record.payload,
                    patch.appearance,
                    &mut meshes,
                    &mut groups,
                    residue,
                )?);
            }
            tag if tag == u32::from(token::TESS_TRIANGLE_FAN)
                || tag == u32::from(token::TESS_TRIANGLE_STRIP)
                || tag == u32::from(token::TESS_TRIANGLE_LIST) =>
            {
                let Some((ordinal, block)) = patch
                    .as_ref()
                    .and_then(|patch| Some((patch.ordinal, patch.block.as_ref()?)))
                else {
                    return Err(CodecError::malformed(format_args!(
                        "triangle primitive at offset {} precedes every vertex block",
                        record.payload.start()
                    )));
                };
                let mesh = meshes
                    .get_mut(block.mesh)
                    .ok_or_else(|| CodecError::malformed("vertex block names no mesh"))?;
                triangles(record, ordinal, block, mesh)?;
            }
            _ if record.is_terminator() => {}
            tag => residue.surface_records.count(tag),
        }
    }
    Ok(meshes)
}

/// Reads a patch header and resolves its appearance (§6.2).
fn patch_appearance(
    record: Record<'_>,
    appearances: &AppearanceTable,
    residue: &mut MemberResidue,
) -> Result<Option<u32>, CodecError> {
    let mut payload = record.payload;
    let head = crate::read::record(&mut payload, patch_header::LEN)?;
    let index = at(head, patch_header::APPEARANCE)?.req_u32_le()?;
    let ref_count = at(head, patch_header::REF_COUNT)?.req_u32_le()?;
    let refs_len = index_from_u32(ref_count).checked_mul(INDEX_LEN);
    if refs_len != Some(payload.remaining()) {
        return Err(CodecError::malformed(format_args!(
            "patch header at offset {} declares {ref_count} references in {} bytes",
            record.payload.start(),
            payload.remaining()
        )));
    }
    residue.patch_headers += 1;
    if index == NO_APPEARANCE {
        return Ok(None);
    }
    let resolved = appearances.resolve(index);
    if resolved.is_none() {
        residue.appearance_unresolved += 1;
    }
    Ok(resolved)
}

/// Reads a vertex block into the mesh of its appearance and layout (§6.3).
fn read_vertices(
    payload: View<'_>,
    appearance: Option<u32>,
    meshes: &mut Vec<Mesh>,
    groups: &mut BTreeMap<(Option<u32>, bool), usize>,
    residue: &mut MemberResidue,
) -> Result<Block, CodecError> {
    let mut view = payload;
    let head = record(&mut view, vertex_block::LEN)?;
    let layout = at(head, vertex_block::LAYOUT)?.req_u8()?;
    let count = at(head, vertex_block::VERTEX_COUNT)?.req_u32_le()?;
    if layout & !(LAYOUT_NORMAL | LAYOUT_TEXTURE) != 0 {
        return Err(CodecError::malformed(format_args!(
            "vertex block at offset {} has undefined layout {layout:#04x}",
            payload.start()
        )));
    }
    let normals = layout & LAYOUT_NORMAL != 0;
    let texture = layout & LAYOUT_TEXTURE != 0;
    let stride =
        VECTOR_LEN + if normals { VECTOR_LEN } else { 0 } + if texture { TEXTURE_LEN } else { 0 };
    let vertices = index_from_u32(count);
    if vertices.checked_mul(stride) != Some(view.remaining()) {
        return Err(CodecError::malformed(format_args!(
            "vertex block at offset {} declares {count} vertices of {stride} bytes in {} bytes",
            payload.start(),
            view.remaining()
        )));
    }
    let next = meshes.len();
    let index = *groups.entry((appearance, normals)).or_insert(next);
    if index == next {
        meshes.push(Mesh {
            appearance,
            normals: normals.then(Vec::new),
            ..Mesh::default()
        });
    }
    let mesh = meshes
        .get_mut(index)
        .ok_or_else(|| CodecError::malformed("vertex block names no mesh"))?;
    let base = u32::try_from(mesh.positions.len())
        .ok()
        .filter(|base| base.checked_add(count).is_some());
    let Some(base) = base else {
        return Err(CodecError::malformed(format_args!(
            "vertex block at offset {} overflows the 32-bit index range of its mesh",
            payload.start()
        )));
    };
    mesh.positions.reserve(vertices);
    for _ in 0..vertices {
        mesh.positions.push(f32x3(&mut view)?);
        if let Some(lane) = mesh.normals.as_mut() {
            lane.push(f32x3(&mut view)?);
        }
        if texture {
            view.req_take(TEXTURE_LEN)?;
        }
    }
    if texture {
        residue.texture_vertices += vertices;
    }
    Ok(Block {
        mesh: index,
        base,
        count,
    })
}

/// Appends the triangles of one primitive record (§6.4).
fn triangles(
    record: Record<'_>,
    patch: usize,
    block: &Block,
    mesh: &mut Mesh,
) -> Result<(), CodecError> {
    let bytes = record.payload.window();
    if bytes.len() % INDEX_LEN != 0 {
        return Err(CodecError::malformed(format_args!(
            "triangle primitive at offset {} has {} bytes, not a multiple of 4",
            record.payload.start(),
            bytes.len()
        )));
    }
    let mut view = record.payload;
    let mut indexes = Vec::with_capacity(bytes.len() / INDEX_LEN);
    while !view.is_empty() {
        let index = view.req_u32_le()?;
        if index >= block.count {
            return Err(CodecError::malformed(format_args!(
                "triangle primitive at offset {} indexes vertex {index} of {}",
                record.payload.start(),
                block.count
            )));
        }
        indexes.push(block.base + index);
    }
    let first = mesh.triangles.len();
    let produced: Vec<[u32; 3]> = match record.tag {
        tag if tag == u32::from(token::TESS_TRIANGLE_FAN) => indexes
            .get(1..)
            .unwrap_or_default()
            .windows(2)
            .map(|pair| [indexes[0], pair[0], pair[1]])
            .collect(),
        tag if tag == u32::from(token::TESS_TRIANGLE_STRIP) => indexes
            .windows(3)
            .enumerate()
            .map(|(k, run)| {
                if k % 2 == 0 {
                    [run[0], run[1], run[2]]
                } else {
                    [run[1], run[0], run[2]]
                }
            })
            .collect(),
        _ => {
            if indexes.len() % 3 != 0 {
                return Err(CodecError::malformed(format_args!(
                    "triangle list at offset {} holds {} indexes, not a multiple of 3",
                    record.payload.start(),
                    indexes.len()
                )));
            }
            indexes
                .chunks_exact(3)
                .map(|run| [run[0], run[1], run[2]])
                .collect()
        }
    };
    mesh.triangles.extend(produced);
    let ordinals = (first..mesh.triangles.len())
        .map(u32::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| CodecError::malformed("mesh triangle count exceeds the 32-bit range"))?;
    if ordinals.is_empty() {
        return Ok(());
    }
    match mesh.patches.last_mut() {
        Some((ordinal, members)) if *ordinal == patch => members.extend(ordinals),
        _ => mesh.patches.push((patch, ordinals)),
    }
    Ok(())
}
