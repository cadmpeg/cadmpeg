// SPDX-License-Identifier: Apache-2.0
//! Author `.paramesh` mesh-geometry containers by the rules of
//! [spec §1.1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#112-mesh-geometry-containers).

use cadmpeg_core::CodecError;

use crate::paramesh::{
    CHANNEL_GROUP, CHANNEL_ROLE, CHANNEL_STREAMS, CHUNK_NAME_TABLE, CHUNK_STREAM,
    FEATURE_EDGE_STREAM, GROUP_GUID, GROUP_KEY, LZMA_PROPERTIES, MAGIC, PROPERTY_KEY,
    PROPERTY_TEXT, RAW_STREAM_MODE, REGISTRY_FACE_GROUP_COUNT, REGISTRY_FEATURE_EDGES,
    REGISTRY_MESH_UUID, REGISTRY_PROPERTY, REGISTRY_TRIANGLES, REGISTRY_TRIANGLE_CHANNEL,
    REGISTRY_VERTEX_CHANNEL, REGISTRY_VERTICES, STREAM_ELEMENT_CODE, STREAM_INDEX, STREAM_VALUES,
    VERSION,
};

/// Version-2 stream descriptors (spec §1.1.2).
#[derive(Clone, Copy)]
pub(super) enum Layout {
    /// `{T: 1, d: 1}`: 32-bit terminal-delta words.
    TerminalDelta,
    /// `{D: 2, T: 3}`: two f32 components.
    Float2,
    /// `{D: 3, T: 3}`: three f32 components.
    Float3,
    /// `{D: 4, T: 3}`: four f32 components.
    Float4,
    /// `{D: 3, T: 3, U: true}`: a direction as two octahedral f32 values.
    PackedDirection,
}

impl Layout {
    fn descriptor(self) -> &'static [u8] {
        match self {
            Self::TerminalDelta => &[0x82, 0xa1, b'T', 1, 0xa1, b'd', 1],
            Self::Float2 => &[0x82, 0xa1, b'D', 2, 0xa1, b'T', 3],
            Self::Float3 => &[0x82, 0xa1, b'D', 3, 0xa1, b'T', 3],
            Self::Float4 => &[0x82, 0xa1, b'D', 4, 0xa1, b'T', 3],
            Self::PackedDirection => &[0x83, 0xa1, b'D', 3, 0xa1, b'T', 3, 0xa1, b'U', 0xc3],
        }
    }

    /// The stream layout of a registry element code, when the code has one.
    pub(super) fn for_element(code: u32) -> Option<Self> {
        match code {
            crate::paramesh::ELEMENT_PAIR_CODE => Some(Self::Float2),
            crate::paramesh::ELEMENT_QUAD_CODE => Some(Self::Float4),
            crate::paramesh::ELEMENT_PACKED_DIRECTION_CODE => Some(Self::PackedDirection),
            crate::paramesh::ELEMENT_TRIANGLE_DELTA_CODE => Some(Self::TerminalDelta),
            _ => None,
        }
    }
}

/// Which registry field declares a channel.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ChannelDomain {
    /// Field 4 without an index stream: one value per vertex.
    Vertex,
    /// Field 4 with an index stream: per-vertex defaults and corner overrides.
    Corner,
    /// Field 5: one value per triangle.
    Triangle,
}

/// One registry channel and its streams.
pub(super) struct Channel {
    pub(super) domain: ChannelDomain,
    pub(super) role: u32,
    pub(super) element_code: u32,
    pub(super) values: Vec<u8>,
    /// Strictly increasing corner positions of the override values, for a
    /// corner channel.
    pub(super) positions: Vec<u32>,
    /// Field-6 face-group records.
    pub(super) groups: Vec<(u32, String)>,
}

/// Everything one container stores.
pub(super) struct Container<'a> {
    pub(super) fusion_uuid: &'a str,
    pub(super) mesh_uuid: &'a str,
    /// Container-space coordinates of each vertex.
    pub(super) vertices: &'a [[f32; 3]],
    pub(super) triangles: &'a [[u32; 3]],
    pub(super) feature_edges: &'a [[u32; 2]],
    pub(super) channels: Vec<Channel>,
}

fn varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = u8::try_from(value & 0x7f).unwrap_or_default();
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn varint_field(out: &mut Vec<u8>, field: u64, value: u64) {
    varint(out, field << 3);
    varint(out, value);
}

fn bytes_field(out: &mut Vec<u8>, field: u64, value: &[u8]) {
    varint(out, (field << 3) | 2);
    varint(out, cadmpeg_core::decode::u64_from_index(value.len()));
    out.extend_from_slice(value);
}

/// MessagePack string: fixstr or str8 (spec §1.1.2 name tables).
fn message_pack_string(out: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    let length = value.len();
    if let Ok(short) = u8::try_from(length) {
        if short < 32 {
            out.push(0xa0 | short);
        } else {
            out.extend_from_slice(&[0xd9, short]);
        }
        out.extend_from_slice(value.as_bytes());
        return Ok(());
    }
    Err(CodecError::InvalidInput(
        "paramesh stream name exceeds 255 bytes".into(),
    ))
}

fn message_pack_integer(out: &mut Vec<u8>, value: u32) {
    if let Ok(small) = u8::try_from(value) {
        if small < 0x80 {
            out.push(small);
        } else {
            out.extend_from_slice(&[0xcc, small]);
        }
    } else if let Ok(medium) = u16::try_from(value) {
        out.push(0xcd);
        out.extend_from_slice(&medium.to_be_bytes());
    } else {
        out.push(0xce);
        out.extend_from_slice(&value.to_be_bytes());
    }
}

/// Encode values as terminal-delta words: each word before the last is the
/// two's-complement difference to the next value, and the last word is the
/// final value. `None` when a difference leaves the i32 range.
pub(super) fn terminal_delta(values: &[u32]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(values.len().checked_mul(4)?);
    for pair in values.windows(2) {
        let delta = i32::try_from(i64::from(pair[1]) - i64::from(pair[0])).ok()?;
        out.extend_from_slice(&delta.to_le_bytes());
    }
    if let Some(last) = values.last() {
        out.extend_from_slice(&last.to_le_bytes());
    }
    Some(out)
}

/// Decode terminal-delta words; the inverse of [`terminal_delta`].
pub(super) fn terminal_delta_values(words: &[u8]) -> Option<Vec<u32>> {
    let words: Vec<u32> = words
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
        .collect();
    let (terminal, deltas) = words.split_last()?;
    let total: i64 = deltas.iter().map(|word| i64::from(word.cast_signed())).sum();
    let mut current = i64::from(*terminal).checked_sub(total)?;
    let mut values = Vec::with_capacity(words.len());
    for word in deltas {
        values.push(u32::try_from(current).ok()?);
        current = current.checked_add(i64::from(word.cast_signed()))?;
    }
    values.push(u32::try_from(current).ok()?);
    Some(values)
}

/// Encode the triangle-corner stream. The first corner is implicit: the
/// decoder recovers it as the unique start that keeps every corner inside the
/// vertex domain, so the corners must reach both vertex 0 and the last
/// vertex. The final word stores the last corner. `None` when the corners do
/// not determine their start or a difference leaves the i32 range.
fn corner_stream(triangles: &[[u32; 3]], vertex_count: usize) -> Option<Vec<u8>> {
    let corners: Vec<u32> = triangles.iter().flatten().copied().collect();
    let last_vertex = u32::try_from(vertex_count.checked_sub(1)?).ok()?;
    if corners.iter().min() != Some(&0) || corners.iter().max() != Some(&last_vertex) {
        return None;
    }
    terminal_delta(&corners)
}

struct Streams {
    names: Vec<(String, u32)>,
    chunks: Vec<u8>,
}

impl Streams {
    fn push(&mut self, name: &str, layout: Layout, data: &[u8]) -> Result<(), CodecError> {
        let id = u32::try_from(self.names.len())
            .map_err(|_| CodecError::InvalidInput("paramesh stream count exceeds u32".into()))?;
        self.names.push((name.to_owned(), id));
        let descriptor = layout.descriptor();
        let mut body = Vec::new();
        body.extend_from_slice(
            &u16::try_from(descriptor.len())
                .map_err(|_| CodecError::malformed("paramesh descriptor exceeds u16"))?
                .to_le_bytes(),
        );
        body.extend_from_slice(descriptor);
        body.extend_from_slice(
            &u32::try_from(data.len())
                .map_err(|_| CodecError::InvalidInput("paramesh stream exceeds u32 bytes".into()))?
                .to_le_bytes(),
        );
        body.extend_from_slice(&[LZMA_PROPERTIES, RAW_STREAM_MODE]);
        body.extend_from_slice(data);
        self.chunks
            .extend_from_slice(&cadmpeg_core::decode::u64_from_index(body.len()).to_le_bytes());
        self.chunks.extend_from_slice(&CHUNK_STREAM.to_le_bytes());
        self.chunks.extend_from_slice(&body);
        Ok(())
    }
}

/// Serialize one container. `None` when the triangle corners cannot be
/// stored (spec §1.1.2: the implicit first corner must be determined).
pub(super) fn encode(container: &Container<'_>) -> Result<Option<Vec<u8>>, CodecError> {
    let Some(corners) = corner_stream(container.triangles, container.vertices.len()) else {
        return Ok(None);
    };
    let mut streams = Streams {
        names: Vec::new(),
        chunks: Vec::new(),
    };
    let mut registry = Vec::new();
    let mut property = Vec::new();
    bytes_field(&mut property, PROPERTY_KEY, b"fusion_uuid");
    bytes_field(&mut property, PROPERTY_TEXT, container.fusion_uuid.as_bytes());
    bytes_field(&mut registry, REGISTRY_PROPERTY, &property);
    let group_count = container
        .channels
        .iter()
        .find(|channel| {
            channel.role == 0
                && channel.domain == ChannelDomain::Triangle
                && channel.element_code == crate::paramesh::ELEMENT_TRIANGLE_DELTA_CODE
        })
        .map_or(0, |channel| channel.groups.len());
    varint_field(
        &mut registry,
        REGISTRY_FACE_GROUP_COUNT,
        cadmpeg_core::decode::u64_from_index(group_count),
    );
    bytes_field(
        &mut registry,
        REGISTRY_MESH_UUID,
        container.mesh_uuid.as_bytes(),
    );
    bytes_field(&mut registry, REGISTRY_VERTICES, b"v");
    bytes_field(&mut registry, REGISTRY_TRIANGLES, b"t");
    let positions: Vec<u8> = container
        .vertices
        .iter()
        .flatten()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    streams.push("v", Layout::Float3, &positions)?;
    streams.push("t", Layout::TerminalDelta, &corners)?;
    for (ordinal, channel) in container.channels.iter().enumerate() {
        let Some(layout) = Layout::for_element(channel.element_code) else {
            return Err(CodecError::NotImplemented(format!(
                "paramesh element code {} has no stream layout",
                channel.element_code
            )));
        };
        let values_name = format!("c{ordinal}");
        let mut stream_entry = Vec::new();
        varint_field(
            &mut stream_entry,
            STREAM_ELEMENT_CODE,
            u64::from(channel.element_code),
        );
        bytes_field(&mut stream_entry, STREAM_VALUES, values_name.as_bytes());
        streams.push(&values_name, layout, &channel.values)?;
        if channel.domain == ChannelDomain::Corner {
            let index_name = format!("c{ordinal}i");
            bytes_field(&mut stream_entry, STREAM_INDEX, index_name.as_bytes());
            let words = terminal_delta(&channel.positions).ok_or_else(|| {
                CodecError::NotImplemented("paramesh corner positions leave the i32 range".into())
            })?;
            streams.push(&index_name, Layout::TerminalDelta, &words)?;
        }
        let mut entry = Vec::new();
        varint_field(&mut entry, CHANNEL_ROLE, u64::from(channel.role));
        bytes_field(&mut entry, CHANNEL_STREAMS, &stream_entry);
        for (key, guid) in &channel.groups {
            let mut group = Vec::new();
            varint_field(&mut group, GROUP_KEY, u64::from(*key));
            bytes_field(&mut group, GROUP_GUID, guid.as_bytes());
            bytes_field(&mut entry, CHANNEL_GROUP, &group);
        }
        bytes_field(
            &mut registry,
            if channel.domain == ChannelDomain::Triangle {
                REGISTRY_TRIANGLE_CHANNEL
            } else {
                REGISTRY_VERTEX_CHANNEL
            },
            &entry,
        );
    }
    if !container.feature_edges.is_empty() {
        let endpoints: Vec<u32> = container.feature_edges.iter().flatten().copied().collect();
        let words = terminal_delta(&endpoints).ok_or_else(|| {
            CodecError::NotImplemented("paramesh feature edges leave the i32 range".into())
        })?;
        streams.push("e", Layout::TerminalDelta, &words)?;
        let mut declaration = Vec::new();
        bytes_field(&mut declaration, FEATURE_EDGE_STREAM, b"e");
        bytes_field(&mut registry, REGISTRY_FEATURE_EDGES, &declaration);
    }
    let mut names = Vec::new();
    let count = streams.names.len();
    if let Ok(small) = u8::try_from(count) {
        if small < 16 {
            names.push(0x80 | small);
        } else {
            names.push(0xde);
            names.extend_from_slice(&u16::from(small).to_be_bytes());
        }
    } else {
        names.push(0xde);
        names.extend_from_slice(
            &u16::try_from(count)
                .map_err(|_| {
                    CodecError::InvalidInput("paramesh stream count exceeds u16".into())
                })?
                .to_be_bytes(),
        );
    }
    for (name, id) in &streams.names {
        message_pack_string(&mut names, name)?;
        message_pack_integer(&mut names, *id);
    }
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&[0; 32]);
    bytes.extend_from_slice(&cadmpeg_core::decode::u64_from_index(registry.len()).to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&registry);
    bytes.extend_from_slice(&cadmpeg_core::decode::u64_from_index(names.len()).to_le_bytes());
    bytes.extend_from_slice(&CHUNK_NAME_TABLE.to_le_bytes());
    bytes.extend_from_slice(&names);
    bytes.extend_from_slice(&streams.chunks);
    Ok(Some(bytes))
}
