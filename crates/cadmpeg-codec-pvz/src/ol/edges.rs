// SPDX-License-Identifier: Apache-2.0
//! The edge tessellation stream (§7): the edge index table and one sampled
//! polyline per exact edge.

use cadmpeg_core::decode::{index_from_u32, View};
use cadmpeg_core::CodecError;

use super::tlv;
use super::MemberResidue;
use crate::layout::{
    edge_tessellation_header as edge_header, token, vertex_block_head as vertex_block,
};
use crate::read::{at, f32x3, record};

/// Tag of the edge header that opens an edge group (§7.2); the singular-use
/// record of the exact stream (§8.13) has the same tag.
const EDGE_HEADER: u32 = token::EXACT_SINGULAR_USE;
/// Vertex layout of an edge vertex block: positions only.
const LAYOUT_POSITION: u8 = 0;
/// Bytes of one edge vertex.
const VERTEX_LEN: usize = 12;
/// Bytes of one primitive vertex index.
const INDEX_LEN: usize = 4;

/// The sample polyline of one exact edge, in stored metres.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EdgePolyline {
    /// Index of the exact edge record (§8.15).
    pub(crate) edge: u32,
    pub(crate) points: Vec<[f32; 3]>,
    /// Segments as index pairs into `points`.
    pub(crate) segments: Vec<[u32; 2]>,
}

/// The edge group being read.
struct Group {
    polyline: EdgePolyline,
    has_block: bool,
}

/// Reads the edge tessellation stream.
pub(super) fn read(
    mut stream: View<'_>,
    stream_id: u32,
    residue: &mut MemberResidue,
) -> Result<Vec<EdgePolyline>, CodecError> {
    tlv::stream_index(&mut stream, stream_id)?;
    let mut polylines = Vec::new();
    let mut group: Option<Group> = None;
    while let Some(record) = tlv::next(&mut stream)? {
        match record.tag {
            token::EDGE_TESS_INDEX_TABLE => {
                let mut payload = record.payload;
                let count = payload.req_u32_le()?;
                if index_from_u32(count).checked_mul(INDEX_LEN) != Some(payload.remaining()) {
                    return Err(CodecError::malformed(format_args!(
                        "edge index table at offset {} declares {count} values in {} bytes",
                        record.payload.start(),
                        payload.remaining()
                    )));
                }
                residue.edge_index_tables += 1;
            }
            EDGE_HEADER => {
                let header = record.payload;
                if header.window().len() != edge_header::LEN {
                    return Err(CodecError::malformed(format_args!(
                        "edge header at offset {} has {} bytes, not {}",
                        header.start(),
                        header.window().len(),
                        edge_header::LEN
                    )));
                }
                let edge = at(header, edge_header::EDGE_INDEX)?.req_u32_le()?;
                polylines.extend(group.take().map(|group| group.polyline));
                group = Some(Group {
                    polyline: EdgePolyline {
                        edge,
                        points: Vec::new(),
                        segments: Vec::new(),
                    },
                    has_block: false,
                });
            }
            token::TESS_VERTEX_BLOCK => {
                let current = open(group.as_mut(), record.payload)?;
                if current.has_block {
                    return Err(CodecError::malformed(format_args!(
                        "edge group of edge {} has a second vertex block",
                        current.polyline.edge
                    )));
                }
                current.polyline.points = read_points(record.payload)?;
                current.has_block = true;
            }
            tag if tag == u32::from(token::TESS_LINE_STRIP)
                || tag == u32::from(token::TESS_LINE_SEGMENTS) =>
            {
                let current = open(group.as_mut(), record.payload)?;
                let segments = read_segments(record.payload, tag, &current.polyline)?;
                current.polyline.segments.extend(segments);
            }
            _ if record.is_terminator() => {}
            tag => residue.edge_records.count(tag),
        }
    }
    polylines.extend(group.map(|group| group.polyline));
    Ok(polylines)
}

/// The open edge group, or a refusal for a record outside every group.
fn open<'g>(group: Option<&'g mut Group>, payload: View<'_>) -> Result<&'g mut Group, CodecError> {
    group.ok_or_else(|| {
        CodecError::malformed(format_args!(
            "edge record at offset {} precedes every edge header",
            payload.start()
        ))
    })
}

fn read_points(payload: View<'_>) -> Result<Vec<[f32; 3]>, CodecError> {
    let mut view = payload;
    let head = record(&mut view, vertex_block::LEN)?;
    let layout = at(head, vertex_block::LAYOUT)?.req_u8()?;
    let count = index_from_u32(at(head, vertex_block::VERTEX_COUNT)?.req_u32_le()?);
    if layout != LAYOUT_POSITION || count.checked_mul(VERTEX_LEN) != Some(view.remaining()) {
        return Err(CodecError::malformed(format_args!(
            "edge vertex block at offset {} has layout {layout} and {count} vertices in {} bytes",
            payload.start(),
            view.remaining()
        )));
    }
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        points.push(f32x3(&mut view)?);
    }
    Ok(points)
}

fn read_segments(
    payload: View<'_>,
    tag: u32,
    polyline: &EdgePolyline,
) -> Result<Vec<[u32; 2]>, CodecError> {
    let mut view = payload;
    let mut indexes = Vec::with_capacity(payload.window().len() / INDEX_LEN);
    while !view.is_empty() {
        let index = view.req_u32_le()?;
        if index_from_u32(index) >= polyline.points.len() {
            return Err(CodecError::malformed(format_args!(
                "line primitive at offset {} indexes point {index} of {} on edge {}",
                payload.start(),
                polyline.points.len(),
                polyline.edge
            )));
        }
        indexes.push(index);
    }
    if tag == u32::from(token::TESS_LINE_STRIP) {
        return Ok(indexes.windows(2).map(|pair| [pair[0], pair[1]]).collect());
    }
    if indexes.len() % 2 != 0 {
        return Err(CodecError::malformed(format_args!(
            "line segments at offset {} hold an odd number of indexes",
            payload.start()
        )));
    }
    Ok(indexes
        .chunks_exact(2)
        .map(|pair| [pair[0], pair[1]])
        .collect())
}
