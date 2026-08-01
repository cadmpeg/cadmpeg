// SPDX-License-Identifier: Apache-2.0
//! Decode `.paramesh` mesh-geometry containers
//! ([spec §1.1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#112-mesh-geometry-containers)).
//!
//! One container holds one mesh body's geometry: a protobuf stream registry
//! followed by chunks, of which the `v` stream holds one f32 coordinate triple
//! per vertex and the `t` stream holds one delta-coded u32 per triangle corner.
//! The `r0`, `r0i`, `r1`, `r2`, `r3`, and `r4` streams are undecoded (PM-01).
//! Vertex coordinates are container coordinates; the mesh-body Design record
//! stores the scale that relates them to model space.

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::le::{u16_at, u32_at, u64_at};

/// Container magic.
const MAGIC: [u8; 12] = [
    0x89, 0x55, 0x44, 0x50, 0x4D, 0x45, 0x53, 0x48, 0x0D, 0x0A, 0x1A, 0x0A,
];
/// The only defined container version.
const VERSION: u32 = 2;
/// Byte offset of the u64 protobuf byte count.
const PROTOBUF_COUNT_AT: usize = 0x30;
/// Byte offset of the protobuf message.
const PROTOBUF_AT: usize = 0x3C;
/// Chunk kind holding the `MessagePack` stream-name table.
const CHUNK_NAME_TABLE: u32 = 3;
/// Chunk kind holding one compressed stream.
const CHUNK_STREAM: u32 = 4;
/// LZMA1 properties byte: `lc` 3, `lp` 0, `pb` 2.
const LZMA_PROPERTIES: u8 = 0x5D;
/// Base-2 logarithm of the LZMA1 dictionary size.
const LZMA_DICTIONARY_LOG: u8 = 0x14;
/// Largest stream this decoder inflates.
const MAX_STREAM_BYTES: u32 = 64 * 1024 * 1024;
/// Protobuf wire type of a length-delimited field.
const PROTOBUF_LENGTH_DELIMITED: u8 = 2;

/// One mesh body's decoded geometry.
pub(crate) struct MeshContainer {
    /// The ASCII GUID the protobuf message carries as `fusion_uuid`, which the
    /// container's Design-segment GUID record repeats.
    pub(crate) fusion_uuid: String,
    /// One coordinate triple per vertex, in container coordinates.
    pub(crate) vertices: Vec<[f64; 3]>,
    /// Triangle corner indices into `vertices`.
    pub(crate) triangles: Vec<[u32; 3]>,
}

fn malformed(message: impl Into<String>) -> CodecError {
    CodecError::Malformed(message.into())
}

/// Read the ASCII GUID stored under the protobuf `fusion_uuid` key. A map
/// entry is a nested message holding the key and then the value, so the
/// value's length-delimited field header follows the key text.
fn protobuf_fusion_uuid(message: &[u8]) -> Result<String, CodecError> {
    const KEY: &[u8] = b"fusion_uuid";
    let at = message
        .windows(KEY.len())
        .position(|window| window == KEY)
        .ok_or_else(|| malformed("paramesh protobuf message has no fusion_uuid key"))?;
    let mut at = at + KEY.len();
    let tag = *message
        .get(at)
        .ok_or_else(|| malformed("paramesh fusion_uuid key has no value field"))?;
    if tag & 0x07 != PROTOBUF_LENGTH_DELIMITED {
        return Err(malformed("paramesh fusion_uuid value is not a byte string"));
    }
    at += 1;
    let count = usize::from(
        *message
            .get(at)
            .ok_or_else(|| malformed("paramesh fusion_uuid value is truncated"))?,
    );
    at += 1;
    let value = message
        .get(at..at.saturating_add(count))
        .ok_or_else(|| malformed("paramesh fusion_uuid value is truncated"))?;
    let value = std::str::from_utf8(value)
        .map_err(|_| malformed("paramesh fusion_uuid value is not ASCII"))?;
    if !crate::bytes::is_guid_hyphenated(value) {
        return Err(malformed("paramesh fusion_uuid value is not a GUID"));
    }
    Ok(value.to_owned())
}

/// Read one `MessagePack` value, returning the string keys and integer values of
/// a map. Only the encodings a stream-name table uses are accepted.
fn message_pack_name_table(bytes: &[u8]) -> Result<Vec<(String, u64)>, CodecError> {
    fn take_integer(bytes: &[u8], at: &mut usize) -> Result<u64, CodecError> {
        let tag = *bytes
            .get(*at)
            .ok_or_else(|| malformed("paramesh name table is truncated"))?;
        *at += 1;
        let (width, value) = match tag {
            0x00..=0x7f => return Ok(u64::from(tag)),
            0xcc => (1, 0),
            0xcd => (2, 0),
            0xce => (4, 0),
            0xcf => (8, 0),
            _ => {
                return Err(malformed(
                    "paramesh name table holds a non-integer stream id",
                ))
            }
        };
        let raw = bytes
            .get(*at..*at + width)
            .ok_or_else(|| malformed("paramesh name table is truncated"))?;
        *at += width;
        // MessagePack integers are big-endian.
        Ok(raw
            .iter()
            .fold(value, |total, byte| (total << 8) | u64::from(*byte)))
    }

    fn take_string(bytes: &[u8], at: &mut usize) -> Result<String, CodecError> {
        let tag = *bytes
            .get(*at)
            .ok_or_else(|| malformed("paramesh name table is truncated"))?;
        *at += 1;
        let count = match tag {
            0xa0..=0xbf => usize::from(tag & 0x1f),
            0xd9 => {
                let count = usize::from(
                    *bytes
                        .get(*at)
                        .ok_or_else(|| malformed("paramesh name table is truncated"))?,
                );
                *at += 1;
                count
            }
            _ => return Err(malformed("paramesh name table holds a non-string key")),
        };
        let raw = bytes
            .get(*at..*at + count)
            .ok_or_else(|| malformed("paramesh name table is truncated"))?;
        *at += count;
        std::str::from_utf8(raw)
            .map(str::to_owned)
            .map_err(|_| malformed("paramesh stream name is not UTF-8"))
    }

    let mut at = 0usize;
    let tag = *bytes
        .get(at)
        .ok_or_else(|| malformed("paramesh name table is empty"))?;
    at += 1;
    let count = match tag {
        0x80..=0x8f => usize::from(tag & 0x0f),
        0xde => {
            let raw = bytes
                .get(at..at + 2)
                .ok_or_else(|| malformed("paramesh name table is truncated"))?;
            at += 2;
            usize::from(u16::from_be_bytes([raw[0], raw[1]]))
        }
        _ => return Err(malformed("paramesh name table is not a MessagePack map")),
    };
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let name = take_string(bytes, &mut at)?;
        entries.push((name, take_integer(bytes, &mut at)?));
    }
    Ok(entries)
}

/// Inflate one kind-4 chunk body: the descriptor map, the uncompressed byte
/// count, the two LZMA1 property bytes, and the raw LZMA1 stream.
fn inflate_stream(body: &[u8]) -> Result<Vec<u8>, CodecError> {
    let descriptor_count = usize::from(
        u16_at(body, 0).ok_or_else(|| malformed("paramesh stream chunk is truncated"))?,
    );
    let at = descriptor_count
        .checked_add(2)
        .ok_or_else(|| malformed("paramesh stream chunk is out of range"))?;
    let declared =
        u32_at(body, at).ok_or_else(|| malformed("paramesh stream chunk is truncated"))?;
    if declared > MAX_STREAM_BYTES {
        return Err(malformed(format!(
            "paramesh stream declares {declared} bytes, above the {MAX_STREAM_BYTES}-byte limit"
        )));
    }
    let properties = body
        .get(at + 4..at + 6)
        .ok_or_else(|| malformed("paramesh stream chunk is truncated"))?;
    if properties != [LZMA_PROPERTIES, LZMA_DICTIONARY_LOG] {
        return Err(malformed(
            "paramesh stream carries undefined LZMA1 properties",
        ));
    }
    let payload = body
        .get(at + 6..)
        .ok_or_else(|| malformed("paramesh stream chunk is truncated"))?;
    // `lzma-rs` reads the properties byte and the four-byte dictionary size
    // from the stream, which the container stores as a properties byte and a
    // base-2 dictionary exponent instead.
    let mut framed = Vec::with_capacity(5 + payload.len());
    framed.push(LZMA_PROPERTIES);
    framed.extend_from_slice(&(1u32 << LZMA_DICTIONARY_LOG).to_le_bytes());
    framed.extend_from_slice(payload);
    let mut out = Vec::with_capacity(declared as usize);
    lzma_rs::lzma_decompress_with_options(
        &mut std::io::Cursor::new(framed.as_slice()),
        &mut out,
        &lzma_rs::decompress::Options {
            unpacked_size: lzma_rs::decompress::UnpackedSize::UseProvided(Some(u64::from(
                declared,
            ))),
            memlimit: Some(1 << usize::from(LZMA_DICTIONARY_LOG)),
            allow_incomplete: false,
        },
    )
    .map_err(|error| malformed(format!("paramesh stream does not decompress: {error}")))?;
    if out.len() != declared as usize {
        return Err(malformed(
            "paramesh stream does not decompress to its declared byte count",
        ));
    }
    Ok(out)
}

/// One f32 coordinate triple per vertex.
fn decode_vertices(stream: &[u8]) -> Result<Vec<[f64; 3]>, CodecError> {
    if !stream.len().is_multiple_of(12) {
        return Err(malformed(
            "paramesh vertex stream is not a whole number of coordinate triples",
        ));
    }
    let mut vertices = Vec::with_capacity(stream.len() / 12);
    for triple in stream.chunks_exact(12) {
        let mut point = [0.0f64; 3];
        for (value, raw) in point.iter_mut().zip(triple.chunks_exact(4)) {
            let component = f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
            if !component.is_finite() {
                return Err(malformed("paramesh vertex coordinate is not finite"));
            }
            *value = f64::from(component);
        }
        vertices.push(point);
    }
    Ok(vertices)
}

/// Resolve the delta-coded corner indices into triangles. Each stored value is
/// the two's-complement difference between consecutive corner indices, the
/// first index is zero, and the last stored value does not continue the
/// sequence.
fn decode_triangles(stream: &[u8], vertices: usize) -> Result<Vec<[u32; 3]>, CodecError> {
    if !stream.len().is_multiple_of(4) || stream.is_empty() {
        return Err(malformed(
            "paramesh corner stream is not a whole number of values",
        ));
    }
    let mut corners = vec![0u32];
    let mut current = 0i64;
    let values = stream.len() / 4;
    for raw in stream.chunks_exact(4).take(values - 1) {
        let delta = i64::from(i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]));
        current += delta;
        let index = u32::try_from(current)
            .map_err(|_| malformed("paramesh corner index is out of range"))?;
        if usize::try_from(index).is_ok_and(|index| index >= vertices) {
            return Err(malformed("paramesh corner index names no vertex"));
        }
        corners.push(index);
    }
    if !corners.len().is_multiple_of(3) {
        return Err(malformed(
            "paramesh corner count is not a whole number of triangles",
        ));
    }
    Ok(corners
        .chunks_exact(3)
        .map(|corner| [corner[0], corner[1], corner[2]])
        .collect())
}

/// Decode one `.paramesh` container entry.
pub(crate) fn decode_mesh_container(bytes: &[u8]) -> Result<MeshContainer, CodecError> {
    if bytes.get(..MAGIC.len()) != Some(&MAGIC[..]) {
        return Err(malformed("paramesh container has no magic"));
    }
    match u32_at(bytes, MAGIC.len()) {
        Some(VERSION) => {}
        _ => return Err(malformed("paramesh container declares an unknown version")),
    }
    let protobuf_count = usize::try_from(
        u64_at(bytes, PROTOBUF_COUNT_AT)
            .ok_or_else(|| malformed("paramesh container is truncated"))?,
    )
    .map_err(|_| malformed("paramesh protobuf message is out of range"))?;
    let protobuf_end = PROTOBUF_AT
        .checked_add(protobuf_count)
        .ok_or_else(|| malformed("paramesh protobuf message is out of range"))?;
    let message = bytes
        .get(PROTOBUF_AT..protobuf_end)
        .ok_or_else(|| malformed("paramesh protobuf message is truncated"))?;
    let fusion_uuid = protobuf_fusion_uuid(message)?;

    let mut at = protobuf_end;
    let mut name_table: Option<Vec<(String, u64)>> = None;
    let mut streams = Vec::new();
    while at < bytes.len() {
        let body_count = usize::try_from(
            u64_at(bytes, at).ok_or_else(|| malformed("paramesh chunk header is truncated"))?,
        )
        .map_err(|_| malformed("paramesh chunk is out of range"))?;
        let kind =
            u32_at(bytes, at + 8).ok_or_else(|| malformed("paramesh chunk header is truncated"))?;
        let body_at = at
            .checked_add(12)
            .ok_or_else(|| malformed("paramesh chunk is out of range"))?;
        let body_end = body_at
            .checked_add(body_count)
            .ok_or_else(|| malformed("paramesh chunk is out of range"))?;
        let body = bytes
            .get(body_at..body_end)
            .ok_or_else(|| malformed("paramesh chunk body is truncated"))?;
        at = body_end;
        match kind {
            CHUNK_NAME_TABLE => {
                if name_table.replace(message_pack_name_table(body)?).is_some() {
                    return Err(malformed("paramesh container repeats its name table"));
                }
            }
            CHUNK_STREAM => streams.push(inflate_stream(body)?),
            _ => {
                return Err(malformed(
                    "paramesh container holds an undefined chunk kind",
                ))
            }
        }
    }
    let mut name_table =
        name_table.ok_or_else(|| malformed("paramesh container has no name table"))?;
    // The kind-4 chunks follow the name table in ascending stream-id order.
    name_table.sort_by_key(|(_, id)| *id);
    let named = |name: &str| {
        name_table
            .iter()
            .position(|(entry, _)| entry == name)
            .and_then(|position| streams.get(position))
    };
    let vertices = decode_vertices(
        named("v").ok_or_else(|| malformed("paramesh container has no vertex stream"))?,
    )?;
    let triangles = decode_triangles(
        named("t").ok_or_else(|| malformed("paramesh container has no corner stream"))?,
        vertices.len(),
    )?;
    Ok(MeshContainer {
        fusion_uuid,
        vertices,
        triangles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One kind-4 chunk: the descriptor map, the uncompressed byte count, the
    /// two LZMA1 property bytes, and the raw LZMA1 stream.
    fn stream_chunk(payload: &[u8]) -> Vec<u8> {
        let mut compressed = Vec::new();
        lzma_rs::lzma_compress(&mut std::io::Cursor::new(payload), &mut compressed)
            .expect("compress stream");
        let mut body = Vec::new();
        // An empty descriptor map.
        body.extend_from_slice(&1u16.to_le_bytes());
        body.push(0x80);
        body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        body.push(LZMA_PROPERTIES);
        body.push(LZMA_DICTIONARY_LOG);
        // `lzma_compress` writes the properties byte, the four-byte dictionary
        // size, and the eight-byte unpacked size ahead of the stream; the
        // container stores none of those three.
        body.extend_from_slice(&compressed[13..]);
        let mut chunk = (body.len() as u64).to_le_bytes().to_vec();
        chunk.extend_from_slice(&CHUNK_STREAM.to_le_bytes());
        chunk.extend_from_slice(&body);
        chunk
    }

    /// One container holding `v` and `t` in that stream-id order.
    fn container(guid: &str, vertices: &[f32], corners: &[i32]) -> Vec<u8> {
        let mut protobuf = vec![0x0a, 11];
        protobuf.extend_from_slice(b"fusion_uuid");
        protobuf.push(0x1a);
        protobuf.push(guid.len() as u8);
        protobuf.extend_from_slice(guid.as_bytes());

        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&[0; 32]);
        bytes.extend_from_slice(&(protobuf.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&protobuf);

        // The name table: a two-entry MessagePack map.
        let mut table = vec![0x82, 0xa1, b'v', 2, 0xa1, b't', 3];
        let mut name_chunk = (table.len() as u64).to_le_bytes().to_vec();
        name_chunk.extend_from_slice(&CHUNK_NAME_TABLE.to_le_bytes());
        name_chunk.append(&mut table);
        bytes.extend_from_slice(&name_chunk);

        let vertex_stream = vertices
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>();
        let corner_stream = corners
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>();
        bytes.extend_from_slice(&stream_chunk(&vertex_stream));
        bytes.extend_from_slice(&stream_chunk(&corner_stream));
        bytes
    }

    const GUID: &str = "8a52d9b8-99b1-4a19-8409-c7c734298305";

    /// Two triangles over four vertices. The first corner index is zero, each
    /// stored value is the difference to the next corner, and the last stored
    /// value does not continue the sequence.
    #[test]
    fn container_decodes_its_vertices_and_delta_coded_corners() {
        let container = container(
            GUID,
            &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0],
            &[1, 1, 1, -2, 1, 7],
        );
        let mesh = decode_mesh_container(&container).expect("mesh container");
        assert_eq!(mesh.fusion_uuid, GUID);
        assert_eq!(mesh.vertices.len(), 4);
        assert_eq!(mesh.vertices[2], [1.0, 1.0, 0.0]);
        assert_eq!(mesh.triangles, [[0, 1, 2], [3, 1, 2]]);
    }

    /// A corner run that does not close into whole triangles is refused rather
    /// than truncated.
    #[test]
    fn container_refuses_a_partial_triangle() {
        let container = container(
            GUID,
            &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
            &[1, 1, 1, 7],
        );
        assert!(decode_mesh_container(&container).is_err());
    }

    /// A corner index naming no vertex is refused.
    #[test]
    fn container_refuses_a_corner_index_beyond_the_vertex_stream() {
        let container = container(
            GUID,
            &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
            &[1, 9, 1, 7],
        );
        assert!(decode_mesh_container(&container).is_err());
    }

    /// A stream whose LZMA1 properties are not the container's own is refused.
    #[test]
    fn container_refuses_undefined_stream_properties() {
        let mut container = container(GUID, &[0.0, 0.0, 0.0], &[0, 0]);
        let at = container
            .windows(2)
            .position(|window| window == [LZMA_PROPERTIES, LZMA_DICTIONARY_LOG])
            .expect("properties");
        container[at] = 0x00;
        assert!(decode_mesh_container(&container).is_err());
    }
}
