// SPDX-License-Identifier: Apache-2.0
//! Bounded `ON_Mesh` decoding.
//!
//! Mesh channel kinds are codec-owned and their payloads are little-endian:
//! [`CHANNEL_UV`] is two `f32`, [`CHANNEL_COLOR`] is four direct `ON_Color`
//! bytes in memory order, [`CHANNEL_SURFACE_PARAMETERS`] is two `f64`,
//! [`CHANNEL_CURVATURE`] is two `f64`, and [`CHANNEL_NGON_GROUP`] is the
//! retained native grouping record. Channel data is never unit-scaled.

use std::ops::Range;

use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::{Tessellation, TessellationChannel};
use flate2::{Decompress, FlushDecompress, Status};

use crate::chunks::{
    chunk_at, verify_checksum, ArchiveVersion, BoundedReader, ChecksumStatus, FramingError,
};
use crate::curves::{error, unsupported, GeometryError};
use crate::wire::Uuid;

/// `ON_Mesh` class UUID.
pub(crate) const ON_MESH: Uuid = Uuid::from_canonical([
    0x4e, 0xd7, 0xd4, 0xe4, 0xe9, 0x47, 0x11, 0xd3, 0xbf, 0xe5, 0x00, 0x10, 0x83, 0x01, 0x22, 0xf0,
]);
/// Codec-owned UV channel kind.
pub(crate) const CHANNEL_UV: u32 = 0x5248_0001;
/// Codec-owned color channel kind.
pub(crate) const CHANNEL_COLOR: u32 = 0x5248_0002;
/// Codec-owned surface-parameter channel kind.
pub(crate) const CHANNEL_SURFACE_PARAMETERS: u32 = 0x5248_0003;
/// Codec-owned curvature channel kind.
pub(crate) const CHANNEL_CURVATURE: u32 = 0x5248_0004;
const MAX_MESH_VERTICES: usize = 1 << 24;
const MAX_MESH_FACES: usize = 1 << 24;
const MAX_BUFFER_OUTPUT: usize = 256 * 1024 * 1024;
const MAX_DOCUMENT_BUFFER_OUTPUT: usize = 256 * 1024 * 1024;

/// Document-wide budget for retained decompressed mesh buffers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MeshBudget {
    used: usize,
    limit: usize,
}

impl MeshBudget {
    /// Creates an empty production document budget.
    pub(crate) fn new() -> Self {
        Self {
            used: 0,
            limit: MAX_DOCUMENT_BUFFER_OUTPUT,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_limit(limit: usize) -> Self {
        Self { used: 0, limit }
    }

    fn charge(&mut self, bytes: usize) -> bool {
        let Some(used) = self.used.checked_add(bytes) else {
            return false;
        };
        if used > self.limit {
            return false;
        }
        self.used = used;
        true
    }
}

/// A decoded mesh and non-fatal channel warnings.
#[derive(Debug, Clone)]
pub(crate) struct DecodedMesh {
    /// Typed IR tessellation.
    pub(crate) tessellation: Tessellation,
    /// Per-object warnings.
    pub(crate) warnings: Vec<String>,
    /// Whether source coordinates were converted to millimeters.
    pub(crate) scaled: bool,
}

/// Caller-owned identity and archive metadata for one mesh decode.
pub(crate) struct MeshDecodeOptions {
    /// Source writer version used by version-gated fields.
    pub(crate) writer_version: Option<i64>,
    /// Source-object association assigned to the tessellation.
    pub(crate) association: Option<cadmpeg_ir::SourceObjectAssociation>,
    /// Deterministic tessellation ID.
    pub(crate) id: String,
    /// Native-unit to millimeter scale.
    pub(crate) scale: f64,
}

#[derive(Default)]
struct MeshChannels {
    vertices: Vec<[f32; 3]>,
    normals: Vec<Vector3>,
    channels: Vec<TessellationChannel>,
    warnings: Vec<String>,
}

/// Returns whether a UUID is `ON_Mesh`.
pub(crate) fn supported_class(uuid: Uuid) -> bool {
    uuid == ON_MESH
}

/// Decodes one bounded `ON_Mesh` class-data payload.
pub(crate) fn decode(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    options: MeshDecodeOptions,
    document_budget: &mut MeshBudget,
) -> Result<DecodedMesh, GeometryError> {
    let checkpoint = *document_budget;
    let result = decode_inner(data, range, archive, options, document_budget);
    if result.is_err() {
        *document_budget = checkpoint;
    }
    result
}

fn decode_inner(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    options: MeshDecodeOptions,
    document_budget: &mut MeshBudget,
) -> Result<DecodedMesh, GeometryError> {
    let MeshDecodeOptions {
        writer_version,
        association,
        id,
        scale,
    } = options;
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let mut decoded = MeshChannels::default();
    let version = reader.u8()?;
    let major = version >> 4;
    let minor = version & 0x0f;
    if major == 2 || major == 0 || major > 3 {
        return Err(unsupported(
            reader.position() - 1,
            "unsupported ON_Mesh major",
        ));
    }
    if minor > 8 {
        return Err(unsupported(
            reader.position() - 1,
            "unsupported ON_Mesh minor",
        ));
    }
    if major == 3 && archive == ArchiveVersion::V5 && minor > 5 {
        return Err(unsupported(
            reader.position() - 1,
            "mesh minor is newer than the V5 writer band",
        ));
    }
    let vertex_count = count(&mut reader, MAX_MESH_VERTICES)?;
    let face_count = count(&mut reader, MAX_MESH_FACES)?;
    for _ in 0..4 {
        interval(&mut reader)?;
    }
    reader.f64()?;
    reader.f64()?;
    for _ in 0..16 {
        reader.f32()?;
    }
    reader.i32()?;
    let parameters_present = reader.u8()?;
    if parameters_present > 1 {
        return Err(error(
            reader.position() - 1,
            "invalid mesh-parameters presence",
        ));
    }
    if parameters_present != 0 {
        consume_optional_chunk(
            &mut reader,
            archive,
            &mut decoded.warnings,
            "mesh parameters",
        )?;
    }
    for _ in 0..4 {
        let present = reader.u8()?;
        if present > 1 {
            return Err(error(
                reader.position() - 1,
                "invalid curvature-stat presence",
            ));
        }
        if present != 0 {
            consume_optional_chunk(
                &mut reader,
                archive,
                &mut decoded.warnings,
                "mesh curvature statistics",
            )?;
        }
    }
    let triangles = read_faces(&mut reader, vertex_count, face_count)?;
    let mut decompressed_bytes = 0;
    if major == 1 {
        read_raw_channels(
            &mut reader,
            vertex_count,
            &mut decoded.vertices,
            &mut decoded.normals,
            &mut decoded.channels,
            &mut decoded.warnings,
        )?;
    } else {
        read_compressed_channels(
            &mut reader,
            vertex_count,
            &mut decoded,
            &mut decompressed_bytes,
            document_budget,
            archive,
        )?;
    }
    if minor >= 2 {
        reader.i32()?;
    }
    if major == 3 && minor >= 3 {
        let _mapping_id = uuid(&mut reader)?;
        let surface = read_buffer(
            &mut reader,
            vertex_count * 16,
            &mut decoded.warnings,
            "surface parameters",
            &mut decompressed_bytes,
            document_budget,
            archive,
        )?;
        if let Some(bytes) = surface {
            decoded
                .channels
                .push(channel(CHANNEL_SURFACE_PARAMETERS, 16, vertex_count, bytes));
        }
    }
    if major == 3 && minor >= 4 && writer_version.is_some_and(|version| version >= 200_606_010) {
        read_mapping_tag(&mut reader, archive, &mut decoded.warnings)?;
    }
    if major == 3 && minor >= 5 {
        for _ in 0..3 {
            let value = reader.u8()?;
            if value > 2 {
                decoded
                    .warnings
                    .push("invalid mesh tri-state flag retained".to_string());
            }
        }
    }
    if major == 3 && minor >= 6 && reader.bool()? {
        read_ngons(
            &mut reader,
            archive,
            vertex_count,
            face_count,
            &mut decoded.warnings,
        )?;
    }
    let mut double_vertices = None;
    if major == 3 && minor >= 7 && reader.bool()? {
        let (count, bytes) = read_double_chunk(
            &mut reader,
            archive,
            &mut decoded.warnings,
            vertex_count,
            &mut decompressed_bytes,
            document_budget,
        )?;
        if count == vertex_count {
            if let Some(bytes) = bytes {
                let values = parse_f64_points(&bytes)?;
                if values
                    .iter()
                    .all(|point| point.iter().all(|v| v.is_finite()))
                    && synchronization_ok(&values, &decoded.vertices)
                {
                    double_vertices = Some(values);
                } else {
                    decoded
                        .warnings
                        .push("double vertices rejected; using float vertices".to_string());
                }
            }
        } else {
            decoded
                .warnings
                .push("double vertex count mismatch; using float vertices".to_string());
        }
    }
    if major == 3 && minor >= 8 {
        for _ in 0..6 {
            reader.f64()?;
        }
    }
    if reader.remaining() != 0 {
        return Err(error(
            reader.position(),
            "ON_Mesh payload has trailing bytes",
        ));
    }
    let source_vertices = double_vertices.unwrap_or_else(|| {
        decoded
            .vertices
            .into_iter()
            .map(|point| {
                [
                    f64::from(point[0]),
                    f64::from(point[1]),
                    f64::from(point[2]),
                ]
            })
            .collect()
    });
    let vertices = source_vertices
        .into_iter()
        .map(|point| {
            Some(Point3::new(
                crate::wire::scaled_coordinate(point[0], scale)?,
                crate::wire::scaled_coordinate(point[1], scale)?,
                crate::wire::scaled_coordinate(point[2], scale)?,
            ))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| error(reader.position(), "scaled mesh vertex is invalid"))?;
    Ok(DecodedMesh {
        tessellation: Tessellation {
            id,
            body: None,
            faces: Vec::new(),
            chordal_deflection: None,
            source_object: association,
            vertices,
            triangles,
            strip_lengths: Vec::new(),
            normals: decoded.normals,
            channels: decoded.channels,
        },
        warnings: decoded.warnings,
        scaled: scale != 1.0,
    })
}

fn read_faces(
    reader: &mut BoundedReader<'_>,
    vertices: usize,
    faces: usize,
) -> Result<Vec<[u32; 3]>, GeometryError> {
    let width = reader.i32()?;
    let expected = if vertices < 256 {
        1
    } else if vertices < 65_536 {
        2
    } else {
        4
    };
    if width != expected {
        return Err(error(
            reader.position() - 4,
            "mesh face index width mismatch",
        ));
    }
    let bytes = faces
        .checked_mul(4)
        .and_then(|value| value.checked_mul(width as usize))
        .ok_or_else(|| error(reader.position(), "mesh face byte count overflow"))?;
    let raw = reader.take(bytes)?;
    let quad_count = (0..faces)
        .filter(|face| {
            let base = face * 4 * width as usize;
            face_index(raw, base + 2 * width as usize, width)
                != face_index(raw, base + 3 * width as usize, width)
        })
        .count();
    let triangle_count = faces
        .checked_add(quad_count)
        .filter(|count| *count <= MAX_MESH_FACES)
        .ok_or_else(|| error(reader.position(), "mesh triangle output budget exceeded"))?;
    let mut result = Vec::with_capacity(triangle_count);
    for face in 0..faces {
        let mut indices = [0_u32; 4];
        for (slot, index) in indices.iter_mut().enumerate() {
            let offset = (face * 4 + slot) * width as usize;
            *index = face_index(raw, offset, width);
            if (*index as usize) >= vertices {
                return Err(error(reader.position(), "mesh face index out of range"));
            }
        }
        if indices[2] == indices[3] {
            result.push([indices[0], indices[1], indices[2]]);
        } else {
            result.push([indices[0], indices[1], indices[2]]);
            result.push([indices[0], indices[2], indices[3]]);
        }
    }
    Ok(result)
}

fn face_index(raw: &[u8], offset: usize, width: i32) -> u32 {
    match width {
        1 => u32::from(raw[offset]),
        2 => u16::from_le_bytes([raw[offset], raw[offset + 1]]) as u32,
        4 => u32::from_le_bytes(raw[offset..offset + 4].try_into().expect("face width")),
        _ => unreachable!(),
    }
}

fn read_raw_channels(
    reader: &mut BoundedReader<'_>,
    vertices: usize,
    points: &mut Vec<[f32; 3]>,
    normals: &mut Vec<Vector3>,
    channels: &mut Vec<TessellationChannel>,
    warnings: &mut Vec<String>,
) -> Result<(), GeometryError> {
    let vertex_bytes = read_counted_raw(reader, vertices, 12, "vertices", warnings)?;
    if let Some(bytes) = vertex_bytes {
        *points = parse_f32_points(&bytes)?;
    }
    let normal_bytes = read_counted_raw(reader, vertices, 12, "normals", warnings)?;
    if let Some(bytes) = normal_bytes {
        match parse_f32_vectors(&bytes) {
            Ok(value) => *normals = value,
            Err(_) => warnings.push("normals channel contains nonfinite values".to_string()),
        }
    }
    let uv = read_counted_raw(reader, vertices, 8, "UV", warnings)?;
    if let Some(bytes) = uv {
        channels.push(channel(CHANNEL_UV, 8, vertices, bytes));
    }
    let curvature = read_counted_raw(reader, vertices, 16, "curvature", warnings)?;
    if let Some(bytes) = curvature {
        channels.push(channel(CHANNEL_CURVATURE, 16, vertices, bytes));
    }
    let colors = read_counted_raw(reader, vertices, 4, "colors", warnings)?;
    if let Some(bytes) = colors {
        channels.push(channel(CHANNEL_COLOR, 4, vertices, bytes));
    }
    if points.len() != vertices {
        return Err(error(reader.position(), "mesh vertex channel is required"));
    }
    Ok(())
}

fn read_compressed_channels(
    reader: &mut BoundedReader<'_>,
    vertices: usize,
    decoded: &mut MeshChannels,
    decompressed_bytes: &mut usize,
    document_budget: &mut MeshBudget,
    archive: ArchiveVersion,
) -> Result<(), GeometryError> {
    let expected = [
        vertices * 12,
        vertices * 12,
        vertices * 8,
        vertices * 16,
        vertices * 4,
    ];
    let names = ["vertices", "normals", "UV", "curvature", "colors"];
    for (index, expected_size) in expected.into_iter().enumerate() {
        let bytes = read_buffer(
            reader,
            expected_size,
            &mut decoded.warnings,
            names[index],
            decompressed_bytes,
            document_budget,
            archive,
        )?;
        let Some(bytes) = bytes else { continue };
        match index {
            0 => decoded.vertices = parse_f32_points(&bytes)?,
            1 => match parse_f32_vectors(&bytes) {
                Ok(value) => decoded.normals = value,
                Err(_) => decoded
                    .warnings
                    .push("normals channel contains nonfinite values".to_string()),
            },
            2 => decoded
                .channels
                .push(channel(CHANNEL_UV, 8, vertices, bytes)),
            3 => decoded
                .channels
                .push(channel(CHANNEL_CURVATURE, 16, vertices, bytes)),
            4 => decoded
                .channels
                .push(channel(CHANNEL_COLOR, 4, vertices, bytes)),
            _ => unreachable!(),
        }
    }
    if decoded.vertices.len() != vertices {
        return Err(error(reader.position(), "mesh vertex channel is required"));
    }
    Ok(())
}

fn read_counted_raw(
    reader: &mut BoundedReader<'_>,
    vertices: usize,
    item_size: usize,
    name: &str,
    warnings: &mut Vec<String>,
) -> Result<Option<Vec<u8>>, GeometryError> {
    let count = reader.i32()?;
    if count < 0 {
        warnings.push(format!("{name} channel has a negative count"));
        return Ok(None);
    }
    if count == 0 {
        return Ok(None);
    }
    let bytes = (count as usize)
        .checked_mul(item_size)
        .ok_or_else(|| error(reader.position(), "mesh channel byte count overflow"))?;
    let data = reader.take(bytes)?.to_vec();
    if count as usize != vertices {
        warnings.push(format!("{name} channel count mismatch"));
        return Ok(None);
    }
    Ok(Some(data))
}

fn read_buffer(
    reader: &mut BoundedReader<'_>,
    expected: usize,
    warnings: &mut Vec<String>,
    name: &str,
    decompressed_bytes: &mut usize,
    document_budget: &mut MeshBudget,
    archive: ArchiveVersion,
) -> Result<Option<Vec<u8>>, GeometryError> {
    let budget_checkpoint = *document_budget;
    let declared = reader.u32()? as usize;
    if declared == 0 {
        return Ok(None);
    }
    if declared > MAX_BUFFER_OUTPUT {
        return Err(error(
            reader.position() - 4,
            &format!("invalid {name} size"),
        ));
    }
    *decompressed_bytes = decompressed_bytes
        .checked_add(declared)
        .filter(|total| *total <= MAX_BUFFER_OUTPUT)
        .ok_or_else(|| {
            error(
                reader.position() - 4,
                "mesh cumulative buffer budget exceeded",
            )
        })?;
    if !document_budget.charge(declared) {
        return Err(error(
            reader.position() - 4,
            "document mesh buffer budget exceeded",
        ));
    }
    let crc = reader.u32()?;
    let method = reader.u8()?;
    let (bytes, consumed) = match method {
        0 => {
            let mut input = reader.unread()?;
            (input.take(declared)?.to_vec(), declared)
        }
        1 => {
            let chunk = chunk_at(
                reader.backing_bytes(),
                reader.position(),
                reader.end(),
                archive,
                false,
            )?;
            if chunk.typecode != 0x4000_8000 || chunk.short {
                return Err(error(
                    reader.position(),
                    "compressed buffer is not anonymous",
                ));
            }
            let input =
                BoundedReader::new(reader.backing_bytes(), chunk.body.start, chunk.body.end)?;
            let (bytes, compressed) = inflate(input, declared)?;
            if compressed != chunk.body.len() {
                return Err(error(
                    chunk.body.start + compressed,
                    "zlib chunk has trailing bytes",
                ));
            }
            if matches!(
                verify_checksum(reader.backing_bytes(), &chunk)?,
                ChecksumStatus::Mismatch { .. }
            ) {
                warnings.push(format!("{name} compressed chunk CRC mismatch"));
            }
            (bytes, chunk.next_offset - reader.position())
        }
        _ => {
            return Err(error(
                reader.position() - 1,
                "unknown compressed-buffer method",
            ))
        }
    };
    reader.skip(consumed)?;
    if bytes.len() != expected {
        warnings.push(format!("{name} compressed-buffer size mismatch"));
        *document_budget = budget_checkpoint;
        return Ok(None);
    }
    if crc32fast::hash(&bytes) != crc {
        warnings.push(format!("{name} compressed-buffer CRC mismatch"));
        *document_budget = budget_checkpoint;
        return Ok(None);
    }
    Ok(Some(bytes))
}

#[cfg(feature = "fuzzing")]
pub(crate) fn fuzz_buffer(data: &[u8]) {
    if data.len() < 2 {
        return;
    }
    let expected = usize::from(u16::from_le_bytes([data[0], data[1]]));
    let Ok(mut reader) = BoundedReader::new(data, 2, data.len()) else {
        return;
    };
    let mut warnings = Vec::new();
    let mut decompressed_bytes = 0;
    let mut document_budget = MeshBudget::new();
    let _ = read_buffer(
        &mut reader,
        expected,
        &mut warnings,
        "fuzz",
        &mut decompressed_bytes,
        &mut document_budget,
        ArchiveVersion::V8,
    );
}

fn inflate(
    mut reader: BoundedReader<'_>,
    expected: usize,
) -> Result<(Vec<u8>, usize), GeometryError> {
    let input = reader.take(reader.remaining())?;
    let mut decoder = Decompress::new(true);
    let mut output = Vec::with_capacity(expected);
    let mut source_offset = 0;
    let mut buffer = [0_u8; 8192];
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder
            .decompress(&input[source_offset..], &mut buffer, FlushDecompress::None)
            .map_err(|_| error(reader.position(), "malformed zlib buffer"))?;
        let consumed = (decoder.total_in() - before_in) as usize;
        source_offset = source_offset
            .checked_add(consumed)
            .ok_or_else(|| error(reader.position(), "zlib input overflow"))?;
        let produced = (decoder.total_out() - before_out) as usize;
        if output
            .len()
            .checked_add(produced)
            .is_none_or(|n| n > expected)
        {
            return Err(error(
                reader.position(),
                "zlib output exceeds declared size",
            ));
        }
        output.extend_from_slice(&buffer[..produced]);
        if status == Status::StreamEnd {
            if output.len() != expected {
                return Err(error(reader.position(), "zlib output size mismatch"));
            }
            return Ok((output, source_offset));
        }
        if consumed == 0 && produced == 0 {
            return Err(error(reader.position(), "truncated zlib buffer"));
        }
    }
}

fn read_ngons(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    vertices: usize,
    faces: usize,
    warnings: &mut Vec<String>,
) -> Result<(), GeometryError> {
    let chunk = chunk_at(
        reader.backing_bytes(),
        reader.position(),
        reader.end(),
        archive,
        false,
    )?;
    push_chunk_checksum_warning(reader.backing_bytes(), &chunk, warnings, "mesh ngon")?;
    let mut child = BoundedReader::new(reader.backing_bytes(), chunk.body.start, chunk.body.end)?;
    let major = child.i32()?;
    let minor = child.i32()?;
    if major != 1 || minor != 0 {
        return Err(unsupported(
            child.position() - 8,
            "unsupported ngon version",
        ));
    }
    let count = checked_u32(&mut child, 1 << 20)?;
    for _ in 0..count {
        let boundary = checked_u32(&mut child, vertices)?;
        if boundary == 0 {
            continue;
        }
        let face_count = checked_u32(&mut child, faces)?;
        for _ in 0..boundary {
            checked_u32(&mut child, vertices)?;
        }
        for _ in 0..face_count {
            checked_u32(&mut child, faces)?;
        }
    }
    if child.remaining() != 0 {
        return Err(error(child.position(), "ngon chunk has trailing bytes"));
    }
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(())
}

fn read_mapping_tag(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<(), GeometryError> {
    let chunk = chunk_at(
        reader.backing_bytes(),
        reader.position(),
        reader.end(),
        archive,
        false,
    )?;
    push_chunk_checksum_warning(reader.backing_bytes(), &chunk, warnings, "mesh mapping tag")?;
    let mut child = BoundedReader::new(reader.backing_bytes(), chunk.body.start, chunk.body.end)?;
    let major = child.i32()?;
    let minor = child.i32()?;
    if major != 1 || minor > 1 {
        return Err(unsupported(
            child.position() - 8,
            "unsupported mapping-tag version",
        ));
    }
    uuid(&mut child)?;
    child.i32()?;
    for _ in 0..16 {
        let value = child.f64()?;
        if !value.is_finite() {
            return Err(error(
                child.position() - 8,
                "mapping transform is not finite",
            ));
        }
    }
    if minor >= 1 {
        child.u32()?;
    }
    if child.remaining() != 0 {
        return Err(error(child.position(), "mapping tag has trailing bytes"));
    }
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(())
}

fn read_double_chunk(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
    vertex_count: usize,
    decompressed_bytes: &mut usize,
    document_budget: &mut MeshBudget,
) -> Result<(usize, Option<Vec<u8>>), GeometryError> {
    let chunk = chunk_at(
        reader.backing_bytes(),
        reader.position(),
        reader.end(),
        archive,
        false,
    )?;
    push_chunk_checksum_warning(
        reader.backing_bytes(),
        &chunk,
        warnings,
        "mesh double vertices",
    )?;
    let mut child = BoundedReader::new(reader.backing_bytes(), chunk.body.start, chunk.body.end)?;
    let major = child.i32()?;
    let minor = child.i32()?;
    if major != 1 || minor > 1 {
        return Err(unsupported(
            child.position() - 8,
            "unsupported double-vertex version",
        ));
    }
    let count = checked_u32(&mut child, MAX_MESH_VERTICES)?;
    let expected = count
        .checked_mul(24)
        .ok_or_else(|| error(child.position(), "double-vertex size overflow"))?;
    let bytes = read_buffer(
        &mut child,
        expected,
        warnings,
        "double vertices",
        decompressed_bytes,
        document_budget,
        archive,
    )?;
    if child.remaining() != 0 {
        return Err(error(
            child.position(),
            "double-vertex chunk has trailing bytes",
        ));
    }
    reader.skip(chunk.next_offset - reader.position())?;
    if count != vertex_count {
        return Ok((count, None));
    }
    Ok((count, bytes))
}

fn consume_optional_chunk(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
    label: &str,
) -> Result<(), GeometryError> {
    let bytes = reader.backing_bytes();
    let chunk = chunk_at(bytes, reader.position(), reader.end(), archive, false)?;
    push_chunk_checksum_warning(bytes, &chunk, warnings, label)?;
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(())
}

fn push_chunk_checksum_warning(
    bytes: &[u8],
    chunk: &crate::chunks::Chunk,
    warnings: &mut Vec<String>,
    label: &str,
) -> Result<(), GeometryError> {
    if matches!(
        verify_checksum(bytes, chunk)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push(format!(
            "{label} CRC mismatch at offset {}",
            chunk.header_start
        ));
    }
    Ok(())
}

fn parse_f32_points(bytes: &[u8]) -> Result<Vec<[f32; 3]>, GeometryError> {
    if !bytes.len().is_multiple_of(12) {
        return Err(error(0, "invalid f32 point channel length"));
    }
    let points: Vec<[f32; 3]> = bytes
        .chunks_exact(12)
        .map(|chunk| {
            [
                f32::from_le_bytes(chunk[0..4].try_into().expect("point width")),
                f32::from_le_bytes(chunk[4..8].try_into().expect("point width")),
                f32::from_le_bytes(chunk[8..12].try_into().expect("point width")),
            ]
        })
        .collect();
    if points
        .iter()
        .any(|point| point.iter().any(|value| !value.is_finite()))
    {
        return Err(error(0, "f32 point channel contains nonfinite values"));
    }
    Ok(points)
}

fn parse_f32_vectors(bytes: &[u8]) -> Result<Vec<Vector3>, GeometryError> {
    Ok(parse_f32_points(bytes)?
        .into_iter()
        .map(|p| Vector3::new(p[0] as f64, p[1] as f64, p[2] as f64))
        .collect())
}

fn parse_f64_points(bytes: &[u8]) -> Result<Vec<[f64; 3]>, GeometryError> {
    if !bytes.len().is_multiple_of(24) {
        return Err(error(0, "invalid f64 point channel length"));
    }
    Ok(bytes
        .chunks_exact(24)
        .map(|chunk| {
            [
                f64::from_le_bytes(chunk[0..8].try_into().expect("point width")),
                f64::from_le_bytes(chunk[8..16].try_into().expect("point width")),
                f64::from_le_bytes(chunk[16..24].try_into().expect("point width")),
            ]
        })
        .collect())
}

fn synchronization_ok(double: &[[f64; 3]], float: &[[f32; 3]]) -> bool {
    double.iter().zip(float).all(|(a, b)| {
        let scale = b.iter().copied().map(f32::abs).fold(0.0_f32, f32::max) as f64;
        a.iter()
            .zip(b)
            .all(|(left, right)| (*left - f64::from(*right)).abs() <= scale * 1.0e-6)
    })
}

fn channel(kind: u32, item_size: u32, count: usize, data: Vec<u8>) -> TessellationChannel {
    TessellationChannel {
        item_size,
        kind,
        flags: 0,
        count: count as u32,
        data,
    }
}

fn interval(reader: &mut BoundedReader<'_>) -> Result<(), FramingError> {
    let lo = reader.f64()?;
    let hi = reader.f64()?;
    if !lo.is_finite() || !hi.is_finite() || lo > hi {
        return Err(FramingError::Structural {
            offset: reader.position() - 16,
            message: "invalid mesh interval".to_string(),
        });
    }
    Ok(())
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, FramingError> {
    Ok(Uuid::from_wire(
        reader.take(16)?.try_into().expect("UUID width"),
    ))
}

fn count(reader: &mut BoundedReader<'_>, cap: usize) -> Result<usize, GeometryError> {
    let value = reader.i32()?;
    if value < 0 || value as usize > cap {
        return Err(error(reader.position() - 4, "mesh count exceeds cap"));
    }
    Ok(value as usize)
}

fn checked_u32(reader: &mut BoundedReader<'_>, cap: usize) -> Result<usize, GeometryError> {
    let value = reader.u32()? as usize;
    if value > cap {
        return Err(error(reader.position() - 4, "mesh count exceeds cap"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::write::ZlibEncoder;
    use flate2::Compression;

    use super::*;

    fn chunk(body: &[u8]) -> Vec<u8> {
        let mut result = 0x4000_8000_u32.to_le_bytes().to_vec();
        result.extend(((body.len() + 4) as i64).to_le_bytes());
        result.extend(body);
        result.extend(crc32fast::hash(body).to_le_bytes());
        result
    }

    fn buffer(value: &[u8], method: u8) -> Vec<u8> {
        let mut result = (value.len() as u32).to_le_bytes().to_vec();
        result.extend(crc32fast::hash(value).to_le_bytes());
        result.push(method);
        if method == 0 {
            result.extend(value);
        } else {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(value).expect("zlib write");
            result.extend(chunk(&encoder.finish().expect("zlib finish")));
        }
        result
    }

    fn compressed_mesh() -> Vec<u8> {
        let mut payload = vec![0x30];
        payload.extend(3_i32.to_le_bytes());
        payload.extend(1_i32.to_le_bytes());
        for _ in 0..4 {
            payload.extend(0.0_f64.to_le_bytes());
            payload.extend(1.0_f64.to_le_bytes());
        }
        payload.extend([0; 16]);
        payload.extend([0; 64]);
        payload.extend(0_i32.to_le_bytes());
        payload.extend([0; 5]);
        payload.extend(1_i32.to_le_bytes());
        payload.extend([0, 1, 2, 2]);
        let mut vertices = Vec::new();
        for value in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            vertices.extend(value.to_le_bytes());
        }
        payload.extend(buffer(&vertices, 0));
        for _ in 0..4 {
            payload.extend(0_u32.to_le_bytes());
        }
        payload
    }

    #[test]
    fn stored_buffer_consumes_adjacent_bytes() {
        let mut bytes = buffer(&[1, 2, 3], 0);
        bytes.push(0xaa);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Vec::new();
        let mut budget = 0;
        let mut document_budget = MeshBudget::new();
        assert_eq!(
            read_buffer(
                &mut reader,
                3,
                &mut warnings,
                "test",
                &mut budget,
                &mut document_budget,
                ArchiveVersion::V8,
            )
            .expect("buffer"),
            Some(vec![1, 2, 3])
        );
        assert_eq!(reader.u8().expect("adjacent"), 0xaa);
        assert!(warnings.is_empty());
    }

    #[test]
    fn zlib_buffer_consumes_one_stream_only() {
        let mut bytes = buffer(&[4, 5, 6, 7], 1);
        bytes.extend(buffer(&[8], 0));
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Vec::new();
        let mut budget = 0;
        let mut document_budget = MeshBudget::new();
        assert_eq!(
            read_buffer(
                &mut reader,
                4,
                &mut warnings,
                "test",
                &mut budget,
                &mut document_budget,
                ArchiveVersion::V8,
            )
            .expect("buffer"),
            Some(vec![4, 5, 6, 7])
        );
        assert_eq!(
            read_buffer(
                &mut reader,
                1,
                &mut warnings,
                "test",
                &mut budget,
                &mut document_budget,
                ArchiveVersion::V8,
            )
            .expect("next"),
            Some(vec![8])
        );
    }

    #[test]
    fn crc_mismatch_consumes_boundary_drops_channel_and_rolls_back_budget() {
        let mut bytes = buffer(&[1, 2], 0);
        bytes[4..8].copy_from_slice(&0_u32.to_le_bytes());
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Vec::new();
        let mut budget = 0;
        let mut document_budget = MeshBudget::new();
        assert_eq!(
            read_buffer(
                &mut reader,
                2,
                &mut warnings,
                "test",
                &mut budget,
                &mut document_budget,
                ArchiveVersion::V8,
            )
            .expect("buffer"),
            None
        );
        assert_eq!(reader.remaining(), 0);
        assert_eq!(warnings.len(), 1);
        assert_eq!(document_budget.used, 0);
    }

    #[test]
    fn bad_method_and_truncated_zlib_fail() {
        let mut bad = vec![1, 0, 0, 0];
        bad.extend(0_u32.to_le_bytes());
        bad.push(9);
        let mut reader = BoundedReader::new(&bad, 0, bad.len()).expect("reader");
        assert!(read_buffer(
            &mut reader,
            1,
            &mut Vec::new(),
            "bad",
            &mut 0,
            &mut MeshBudget::new(),
            ArchiveVersion::V8,
        )
        .is_err());
        let mut truncated = buffer(&[1, 2, 3], 1);
        truncated.truncate(truncated.len() - 2);
        let mut reader = BoundedReader::new(&truncated, 0, truncated.len()).expect("reader");
        assert!(read_buffer(
            &mut reader,
            3,
            &mut Vec::new(),
            "short",
            &mut 0,
            &mut MeshBudget::new(),
            ArchiveVersion::V8,
        )
        .is_err());
    }

    #[test]
    fn output_cap_rejects_before_allocation() {
        let mut bytes = (u32::try_from(MAX_BUFFER_OUTPUT).expect("cap") + 1)
            .to_le_bytes()
            .to_vec();
        bytes.extend([0; 5]);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        assert!(read_buffer(
            &mut reader,
            1,
            &mut Vec::new(),
            "bomb",
            &mut 0,
            &mut MeshBudget::new(),
            ArchiveVersion::V8,
        )
        .is_err());
    }

    #[test]
    fn cumulative_buffer_budget_rejects_another_channel() {
        let bytes = buffer(&[1], 0);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut budget = MAX_BUFFER_OUTPUT;
        assert!(read_buffer(
            &mut reader,
            1,
            &mut Vec::new(),
            "budget",
            &mut budget,
            &mut MeshBudget::new(),
            ArchiveVersion::V8,
        )
        .is_err());
    }

    #[test]
    fn document_buffer_budget_is_shared_across_meshes() {
        let bytes = buffer(&[1], 0);
        let mut document_budget = MeshBudget::with_limit(1);
        for expected_success in [true, false] {
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
            let result = read_buffer(
                &mut reader,
                1,
                &mut Vec::new(),
                "aggregate",
                &mut 0,
                &mut document_budget,
                ArchiveVersion::V8,
            );
            assert_eq!(result.is_ok(), expected_success);
        }
    }

    #[test]
    fn document_budget_rejects_second_complete_mesh() {
        let bytes = compressed_mesh();
        let mut budget = MeshBudget::with_limit(36);
        decode(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: "first".to_string(),
                scale: 1.0,
            },
            &mut budget,
        )
        .expect("first mesh");
        let error = decode(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: "second".to_string(),
                scale: 1.0,
            },
            &mut budget,
        )
        .expect_err("second mesh exceeds aggregate budget");
        assert!(error
            .to_string()
            .contains("document mesh buffer budget exceeded"));
    }

    #[test]
    fn optional_chunks_use_absolute_offsets() {
        let mut bytes = vec![0; 11];
        bytes.extend(chunk(&[1, 2, 3]));
        let end = bytes.len();
        let mut reader = BoundedReader::new(&bytes, 11, end).expect("reader");
        consume_optional_chunk(&mut reader, ArchiveVersion::V5, &mut Vec::new(), "optional")
            .expect("chunk");
        assert_eq!(reader.position(), end);
    }

    #[test]
    fn face_widths_and_quad_split_are_deterministic() {
        for (vertices, width) in [(255_usize, 1_i32), (256, 2), (65_535, 2), (65_536, 4)] {
            let mut bytes = width.to_le_bytes().to_vec();
            for index in [0_u32, 1, 2, 2] {
                match width {
                    1 => bytes.push(index as u8),
                    2 => bytes.extend((index as u16).to_le_bytes()),
                    4 => bytes.extend(index.to_le_bytes()),
                    _ => unreachable!(),
                }
            }
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
            assert_eq!(
                read_faces(&mut reader, vertices, 1).expect("face"),
                vec![[0, 1, 2]]
            );
        }
    }

    #[test]
    fn synchronization_uses_relative_max_coordinate_tolerance() {
        assert!(synchronization_ok(&[[0.0, 0.0, 0.0]], &[[0.0, 0.0, 0.0]]));
        assert!(synchronization_ok(
            &[[1_000_000.0, 0.0, 0.0]],
            &[[1_000_000.5, 0.0, 0.0]]
        ));
        assert!(!synchronization_ok(
            &[[1_000_000.0, 0.0, 0.0]],
            &[[1_002.0, 0.0, 0.0]]
        ));
    }

    #[test]
    fn mapping_and_ngon_chunks_validate_nested_versions() {
        let mut mapping = 1_i32.to_le_bytes().to_vec();
        mapping.extend(1_i32.to_le_bytes());
        mapping.extend([0; 16]);
        mapping.extend(7_i32.to_le_bytes());
        mapping.extend((0..16).flat_map(|_| 1.0_f64.to_le_bytes()));
        mapping.extend(3_u32.to_le_bytes());
        let mapping = chunk(&mapping);
        let mut bytes = vec![0; 3];
        bytes.extend(mapping);
        let end = bytes.len();
        let mut reader = BoundedReader::new(&bytes, 3, end).expect("reader");
        read_mapping_tag(&mut reader, ArchiveVersion::V5, &mut Vec::new()).expect("mapping");

        let mut ngon = 1_i32.to_le_bytes().to_vec();
        ngon.extend(0_i32.to_le_bytes());
        ngon.extend(1_u32.to_le_bytes());
        ngon.extend(3_u32.to_le_bytes());
        ngon.extend([0_u32, 1, 2].into_iter().flat_map(u32::to_le_bytes));
        ngon.extend(1_u32.to_le_bytes());
        let ngon = chunk(&ngon);
        let mut bytes = vec![0; 5];
        bytes.extend(ngon);
        let end = bytes.len();
        let mut reader = BoundedReader::new(&bytes, 5, end).expect("reader");
        read_ngons(&mut reader, ArchiveVersion::V5, 3, 1, &mut Vec::new()).expect("ngon");
    }

    #[test]
    fn nested_mapping_crc_mismatch_warns_and_consumes_boundary() {
        let mut mapping = 1_i32.to_le_bytes().to_vec();
        mapping.extend(1_i32.to_le_bytes());
        mapping.extend([0; 16]);
        mapping.extend(7_i32.to_le_bytes());
        mapping.extend((0..16).flat_map(|_| 1.0_f64.to_le_bytes()));
        mapping.extend(3_u32.to_le_bytes());
        let mut bytes = chunk(&mapping);
        let crc = bytes.len() - 1;
        bytes[crc] ^= 1;
        let end = bytes.len();
        let mut reader = BoundedReader::new(&bytes, 0, end).expect("reader");
        let mut warnings = Vec::new();
        read_mapping_tag(&mut reader, ArchiveVersion::V5, &mut warnings).expect("mapping");
        assert_eq!(reader.position(), end);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("mapping tag CRC mismatch"));
    }

    #[test]
    fn future_v5_mesh_minor_is_retained_unsupported() {
        let bytes = [0x38_u8];
        let result = decode(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: "test".to_string(),
                scale: 1.0,
            },
            &mut MeshBudget::new(),
        );
        assert!(matches!(
            result,
            Err(GeometryError::UnsupportedVersion { .. })
        ));
    }

    #[test]
    fn archive_booleans_reject_reserved_values() {
        let bytes = [2_u8];
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        assert!(reader.bool().is_err());
    }
}
