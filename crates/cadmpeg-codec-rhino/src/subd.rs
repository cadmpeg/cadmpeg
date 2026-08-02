// SPDX-License-Identifier: Apache-2.0
//! Rhino `ON_SubD` control-cage decoding.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;

use cadmpeg_ir::math::Point3;
use cadmpeg_ir::subd::SubdScheme;
use cadmpeg_ir::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdSurface, SubdVertex, SubdVertexTag,
};

use crate::chunks::{
    chunk_at, verify_checksum, ArchiveVersion, BoundedReader, ChecksumStatus, FramingError,
};

/// Canonical `ON_SubD` class UUID.
pub(crate) const ON_SUBD: crate::wire::Uuid = crate::wire::Uuid::from_canonical([
    0xf0, 0x9b, 0xa4, 0xd9, 0x45, 0x5b, 0x42, 0xc3, 0xba, 0x3b, 0xe6, 0xcc, 0xac, 0xef, 0x85, 0x3b,
]);

const ANONYMOUS: u32 = 0x4000_8000;
const MAX_LEVELS: usize = 64;
const MAX_COMPONENTS_PER_LEVEL: usize = 4_000_000;
const MAX_INCIDENT_COMPONENTS: usize = 65_535;
const MAX_SAVED_LIMIT_POINTS: usize = 65_535;

/// A completely decoded `SubD` payload.
#[derive(Debug, Clone)]
pub(crate) enum DecodedSubd {
    /// The outer object explicitly contains no `SubDimple`.
    Empty,
    /// A validated level-zero Catmull-Clark control cage.
    Surface {
        /// Materialized level-zero cage.
        surface: Box<SubdSurface>,
        /// Whether valid non-cage metadata was retained without neutral-IR mapping.
        neutral_metadata: bool,
        /// Recoverable nested checksum warnings.
        warnings: Vec<String>,
    },
}

/// A bounded `SubD` payload failure.
#[derive(Debug, Clone)]
pub(crate) enum SubdError {
    /// The payload or nested record uses a future version.
    UnsupportedVersion { offset: usize, message: String },
    /// The bounded payload is malformed.
    Malformed { offset: usize, message: String },
}

impl fmt::Display for SubdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion { offset, message } => {
                write!(formatter, "{message} at byte {offset}")
            }
            Self::Malformed { offset, message } => write!(formatter, "{message} at byte {offset}"),
        }
    }
}

impl std::error::Error for SubdError {}

impl From<FramingError> for SubdError {
    fn from(value: FramingError) -> Self {
        let offset = match &value {
            FramingError::Truncated { offset, .. }
            | FramingError::InvalidLength { offset, .. }
            | FramingError::Structural { offset, .. }
            | FramingError::Overflow { offset }
            | FramingError::OutOfBounds { offset, .. } => *offset,
            _ => 0,
        };
        Self::Malformed {
            offset,
            message: value.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComponentType {
    Vertex,
    Edge,
    Face,
}

#[derive(Debug, Clone, Copy)]
struct ComponentPointer {
    archive_id: u32,
    direction: bool,
}

#[derive(Debug, Clone)]
struct ComponentBase {
    archive_id: u32,
}

#[derive(Debug, Clone)]
struct RawVertex {
    base: ComponentBase,
    point: Point3,
    tag: u8,
    edges: Vec<ComponentPointer>,
    faces: Vec<ComponentPointer>,
}

#[derive(Debug, Clone)]
struct RawEdge {
    base: ComponentBase,
    tag: u8,
    sector_coefficients: [f64; 2],
    sharpness: [f64; 2],
    vertices: [ComponentPointer; 2],
    faces: Vec<ComponentPointer>,
}

#[derive(Debug, Clone)]
struct RawFace {
    base: ComponentBase,
    edges: Vec<ComponentPointer>,
}

#[derive(Debug, Clone)]
struct RawLevel {
    vertices: Vec<RawVertex>,
    edges: Vec<RawEdge>,
    faces: Vec<RawFace>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Addition {
    Absent,
    Present,
    End,
}

/// Returns whether `class_uuid` names `ON_SubD`.
pub(crate) fn supported_class(class_uuid: crate::wire::Uuid) -> bool {
    class_uuid == ON_SUBD
}

/// Decodes one bounded `ON_SubD` class payload.
pub(crate) fn decode(
    data: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    scale: f64,
    id: cadmpeg_ir::ids::SubdId,
) -> Result<DecodedSubd, SubdError> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(malformed(range.start, "invalid SubD unit scale"));
    }
    let mut reader = BoundedReader::new(data, range.start, range.end)?;
    let mut warnings = Vec::new();
    let has_subdimple = reader.u8()?;
    match has_subdimple {
        0 => {
            finish_payload(&reader)?;
            Ok(DecodedSubd::Empty)
        }
        1 => {
            let chunk = anonymous_chunk(&reader, archive, "SubDimple")?;
            let mut child = BoundedReader::new(data, chunk.body.start, chunk.body.end)?;
            let major = child.i32()?;
            let minor = child.i32()?;
            if major != 1 || !(0..=4).contains(&minor) {
                return Err(SubdError::UnsupportedVersion {
                    offset: chunk.body.start,
                    message: format!("unsupported SubDimple version {major}.{minor}"),
                });
            }
            let (surface, level_count) =
                read_subdimple(&mut child, archive, minor, scale, id, &mut warnings)?;
            finish_chunk(&mut reader, &chunk, child, &mut warnings)?;
            finish_payload(&reader)?;
            Ok(DecodedSubd::Surface {
                surface: Box::new(surface),
                neutral_metadata: minor > 0 || level_count > 1,
                warnings,
            })
        }
        value => Err(malformed(
            range.start,
            format!("invalid has_subdimple value {value}"),
        )),
    }
}

fn read_subdimple(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    minor: i32,
    scale: f64,
    id: cadmpeg_ir::ids::SubdId,
    warnings: &mut Vec<String>,
) -> Result<(SubdSurface, usize), SubdError> {
    let level_count = capped_u32(reader, MAX_LEVELS, "SubD level count")?;
    reader.u32()?;
    reader.u32()?;
    reader.u32()?;
    read_finite_values(reader, 6, "SubD global bounding box")?;

    let mut level_zero = None;
    for expected_level in 0..level_count {
        let level = read_level(reader, archive, expected_level, warnings)?;
        validate_level(&level, expected_level)?;
        if expected_level == 0 {
            level_zero = Some(level);
        }
    }

    if minor >= 1 {
        reader.u8()?;
        read_mapping_tag(reader, archive, warnings)?;
    }
    if minor >= 2 {
        read_symmetry(reader, archive, warnings)?;
    }
    if minor >= 3 {
        reader.u64()?;
    }
    if minor >= 4 {
        reader.bool()?;
        reader.take(16)?;
        reader.bool()?;
        read_subd_hash(reader, archive, warnings)?;
    }

    let level = level_zero.ok_or_else(|| malformed(reader.position(), "SubD has no level zero"))?;
    Ok((materialize(level, scale, id)?, level_count))
}

fn read_level(
    parent: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    expected_level: usize,
    warnings: &mut Vec<String>,
) -> Result<RawLevel, SubdError> {
    let chunk = anonymous_chunk(parent, archive, "SubD level")?;
    let mut reader = BoundedReader::new(parent.backing_bytes(), chunk.body.start, chunk.body.end)?;
    let major = reader.i32()?;
    let minor = reader.i32()?;
    if major != 1 || minor != 1 {
        return Err(SubdError::UnsupportedVersion {
            offset: chunk.body.start,
            message: format!("unsupported SubD level version {major}.{minor}"),
        });
    }
    let level_index = usize::from(reader.u16()?);
    if level_index != expected_level {
        return Err(malformed(
            reader.position() - 2,
            format!("SubD level index {level_index} does not equal {expected_level}"),
        ));
    }
    for algorithm in 0..3 {
        if reader.u8()? != 4 {
            return Err(malformed(
                reader.position() - 1,
                format!("SubD algorithm byte {algorithm} is not Catmull-Clark"),
            ));
        }
    }
    read_finite_values(&mut reader, 6, "SubD control bounding box")?;
    let partitions = [reader.u32()?, reader.u32()?, reader.u32()?, reader.u32()?];
    validate_partitions(partitions, reader.position() - 16)?;
    let vertex_count = partition_count(partitions[0], partitions[1])?;
    let edge_count = partition_count(partitions[1], partitions[2])?;
    let face_count = partition_count(partitions[2], partitions[3])?;
    let component_count = vertex_count
        .checked_add(edge_count)
        .and_then(|value| value.checked_add(face_count))
        .ok_or_else(|| malformed(reader.position(), "SubD component count overflow"))?;
    if component_count > MAX_COMPONENTS_PER_LEVEL {
        return Err(malformed(
            reader.position(),
            "SubD level component count exceeds cap",
        ));
    }
    if component_count > reader.remaining() / 10 {
        return Err(malformed(
            reader.position(),
            "SubD component count exceeds bounded minimum record size",
        ));
    }

    let mut vertices = Vec::with_capacity(vertex_count);
    for archive_id in partitions[0]..partitions[1] {
        vertices.push(read_vertex(
            &mut reader,
            archive,
            archive_id,
            level_index,
            warnings,
        )?);
    }
    let mut edges = Vec::with_capacity(edge_count);
    for archive_id in partitions[1]..partitions[2] {
        edges.push(read_edge(
            &mut reader,
            archive,
            archive_id,
            level_index,
            warnings,
        )?);
    }
    let mut faces = Vec::with_capacity(face_count);
    for archive_id in partitions[2]..partitions[3] {
        faces.push(read_face(
            &mut reader,
            archive,
            archive_id,
            level_index,
            warnings,
        )?);
    }
    match reader.u8()? {
        0 => {}
        1 => consume_anonymous(&mut reader, archive, "SubD render mesh", warnings)?,
        value => {
            return Err(malformed(
                reader.position() - 1,
                format!("invalid SubD render-mesh flag {value}"),
            ));
        }
    }
    let level = RawLevel {
        vertices,
        edges,
        faces,
    };
    finish_chunk(parent, &chunk, reader, warnings)?;
    Ok(level)
}

fn read_vertex(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    expected_id: u32,
    level: usize,
    warnings: &mut Vec<String>,
) -> Result<RawVertex, SubdError> {
    let base = read_base(reader, archive, expected_id, level, warnings)?;
    let tag = reader.u8()?;
    if tag > 4 {
        return Err(malformed(reader.position() - 1, "invalid SubD vertex tag"));
    }
    let point = point(reader, "SubD control point")?;
    let edge_count = usize::from(reader.u16()?);
    let face_count = usize::from(reader.u16()?);
    if edge_count > MAX_INCIDENT_COMPONENTS || face_count > MAX_INCIDENT_COMPONENTS {
        return Err(malformed(
            reader.position() - 4,
            "SubD vertex incidence exceeds cap",
        ));
    }
    let saved_limit_marker = reader.u8()?;
    if saved_limit_marker != 0 {
        let limit_count = capped_u32(
            reader,
            face_count.min(MAX_SAVED_LIMIT_POINTS),
            "saved SubD limit-point count",
        )?;
        if limit_count == 0 {
            return Err(malformed(
                reader.position() - 4,
                "saved SubD limit-point list is empty",
            ));
        }
        for _ in 0..limit_count {
            read_finite_values(reader, 12, "saved SubD limit point")?;
            read_pointer(reader, true)?;
        }
    }
    let serialized_edges = usize::from(reader.u16()?);
    if serialized_edges != edge_count {
        return Err(malformed(
            reader.position() - 2,
            "SubD vertex serialized edge count disagrees",
        ));
    }
    let edges = read_pointers(reader, edge_count, false)?;
    let serialized_faces = usize::from(reader.u16()?);
    if serialized_faces != face_count {
        return Err(malformed(
            reader.position() - 2,
            "SubD vertex serialized face count disagrees",
        ));
    }
    let faces = read_pointers(reader, face_count, false)?;
    read_record_end(reader, archive, warnings)?;
    Ok(RawVertex {
        base,
        point,
        tag,
        edges,
        faces,
    })
}

fn read_edge(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    expected_id: u32,
    level: usize,
    warnings: &mut Vec<String>,
) -> Result<RawEdge, SubdError> {
    let base = read_base(reader, archive, expected_id, level, warnings)?;
    let tag = reader.u8()?;
    if !matches!(tag, 0 | 1 | 2 | 4) {
        return Err(malformed(reader.position() - 1, "invalid SubD edge tag"));
    }
    let face_count = usize::from(reader.u16()?);
    let sector_coefficients = [reader.f64()?, reader.f64()?];
    if sector_coefficients.iter().any(|value| !value.is_finite()) {
        return Err(malformed(
            reader.position() - 16,
            "SubD edge sector coefficient is not finite",
        ));
    }
    let start = reader.f64()?;
    validate_sharpness(start, reader.position() - 8)?;
    if reader.u16()? != 2 {
        return Err(malformed(
            reader.position() - 2,
            "SubD edge vertex count is not two",
        ));
    }
    let endpoint_list = read_pointers(reader, 2, false)?;
    let vertices = [endpoint_list[0], endpoint_list[1]];
    let serialized_faces = usize::from(reader.u16()?);
    if serialized_faces != face_count {
        return Err(malformed(
            reader.position() - 2,
            "SubD edge serialized face count disagrees",
        ));
    }
    let faces = read_pointers(reader, face_count, false)?;
    let mut sharpness = [start, start];
    if archive.value() < 70 {
        expect_zero(reader, "SubD edge end marker")?;
    } else {
        if archive.value() >= 80 {
            match reader.u8()? {
                8 => {
                    sharpness[1] = reader.f64()?;
                    validate_sharpness(sharpness[1], reader.position() - 8)?;
                }
                255 => {
                    return Ok(RawEdge {
                        base,
                        tag,
                        sector_coefficients,
                        sharpness,
                        vertices,
                        faces,
                    });
                }
                value => {
                    return Err(malformed(
                        reader.position() - 1,
                        format!("invalid SubD end-sharpness addition size {value}"),
                    ));
                }
            }
        }
        finish_additions(reader, archive, warnings)?;
    }
    Ok(RawEdge {
        base,
        tag,
        sector_coefficients,
        sharpness,
        vertices,
        faces,
    })
}

fn read_face(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    expected_id: u32,
    level: usize,
    warnings: &mut Vec<String>,
) -> Result<RawFace, SubdError> {
    let base = read_base(reader, archive, expected_id, level, warnings)?;
    reader.u32()?;
    reader.u32()?;
    let edge_count = usize::from(reader.u16()?);
    let serialized_edges = usize::from(reader.u16()?);
    if serialized_edges != edge_count {
        return Err(malformed(
            reader.position() - 2,
            "SubD face serialized edge count disagrees",
        ));
    }
    let edges = read_pointers(reader, edge_count, false)?;
    if archive.value() < 70 {
        expect_zero(reader, "SubD face end marker")?;
    } else {
        match consume_known_addition(reader, archive, 34, "SubD face packing rectangle", warnings)?
        {
            Addition::End => return Ok(RawFace { base, edges }),
            Addition::Absent => {}
            Addition::Present => {
                reader.skip(2)?;
                read_finite_values(reader, 4, "SubD face packing rectangle")?;
            }
        }
        match consume_known_addition(reader, archive, 4, "SubD face material channel", warnings)? {
            Addition::End => return Ok(RawFace { base, edges }),
            Addition::Absent => {}
            Addition::Present => {
                reader.u32()?;
            }
        }
        match consume_known_addition(reader, archive, 4, "SubD face color", warnings)? {
            Addition::End => return Ok(RawFace { base, edges }),
            Addition::Absent => {}
            Addition::Present => {
                reader.u32()?;
            }
        }
        match consume_known_addition(reader, archive, 4, "SubD face pack ID", warnings)? {
            Addition::End => return Ok(RawFace { base, edges }),
            Addition::Absent => {}
            Addition::Present => {
                reader.u32()?;
            }
        }
        match consume_known_addition(reader, archive, 4, "SubD face texture points", warnings)? {
            Addition::End => return Ok(RawFace { base, edges }),
            Addition::Absent => {}
            Addition::Present => {
                let ten_count = capped_u32(reader, edge_count / 10, "SubD texture chunk count")?;
                if ten_count != edge_count / 10 {
                    return Err(malformed(
                        reader.position() - 4,
                        "SubD texture chunk count disagrees with edge count",
                    ));
                }
                for _ in 0..ten_count {
                    if reader.u8()? != 240 {
                        return Err(malformed(
                            reader.position() - 1,
                            "SubD ten-point addition size is not 240",
                        ));
                    }
                    read_finite_values(reader, 30, "SubD texture points")?;
                }
                let remainder = edge_count % 10;
                if remainder > 0 {
                    let expected = u8::try_from(remainder * 24)
                        .map_err(|_| malformed(reader.position(), "texture remainder overflow"))?;
                    if reader.u8()? != expected {
                        return Err(malformed(
                            reader.position() - 1,
                            "SubD texture remainder size disagrees",
                        ));
                    }
                    read_finite_values(reader, remainder * 3, "SubD texture points")?;
                }
            }
        }
        finish_additions(reader, archive, warnings)?;
    }
    Ok(RawFace { base, edges })
}

fn read_base(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    expected_id: u32,
    expected_level: usize,
    warnings: &mut Vec<String>,
) -> Result<ComponentBase, SubdError> {
    let archive_id = reader.u32()?;
    if archive_id != expected_id {
        return Err(malformed(
            reader.position() - 4,
            format!("SubD component archive ID {archive_id} does not equal {expected_id}"),
        ));
    }
    reader.u32()?;
    let subdivision_level = reader.u16()?;
    if usize::from(subdivision_level) != expected_level {
        return Err(malformed(
            reader.position() - 2,
            "SubD component subdivision level disagrees with its level",
        ));
    }
    if archive.value() < 70 {
        let saved_size = reader.u8()?;
        if !matches!(saved_size, 0 | 4) {
            return Err(malformed(
                reader.position() - 1,
                "invalid saved subdivision-point size",
            ));
        }
        if saved_size != 0 {
            read_finite_values(reader, 3, "saved subdivision point")?;
        }
        let deprecated_size = reader.u8()?;
        if !matches!(deprecated_size, 0 | 4) {
            return Err(malformed(
                reader.position() - 1,
                "invalid deprecated SubD vector size",
            ));
        }
        if deprecated_size != 0 {
            read_finite_values(reader, 3, "deprecated SubD vector")?;
        }
    } else {
        match consume_known_addition(reader, archive, 24, "SubD displacement", warnings)? {
            Addition::End => return Ok(ComponentBase { archive_id }),
            Addition::Absent => {}
            Addition::Present => read_finite_values(reader, 3, "deprecated SubD displacement")?,
        }
        match consume_known_addition(reader, archive, 4, "SubD group ID", warnings)? {
            Addition::End => return Ok(ComponentBase { archive_id }),
            Addition::Absent => {}
            Addition::Present => {
                reader.u32()?;
            }
        }
        match consume_known_addition(reader, archive, 5, "SubD symmetry-next", warnings)? {
            Addition::End => return Ok(ComponentBase { archive_id }),
            Addition::Absent => {}
            Addition::Present => read_untyped_pointer(reader)?,
        }
        finish_additions(reader, archive, warnings)?;
    }
    Ok(ComponentBase { archive_id })
}

fn consume_known_addition(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    expected: u8,
    label: &str,
    warnings: &mut Vec<String>,
) -> Result<Addition, SubdError> {
    loop {
        match reader.u8()? {
            0 => return Ok(Addition::Absent),
            value if value == expected => return Ok(Addition::Present),
            254 => consume_anonymous(reader, archive, label, warnings)?,
            255 => return Ok(Addition::End),
            value => {
                return Err(malformed(
                    reader.position() - 1,
                    format!("invalid {label} addition size {value}"),
                ));
            }
        }
    }
}

fn finish_additions(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<(), SubdError> {
    loop {
        match reader.u8()? {
            255 => return Ok(()),
            254 => consume_anonymous(reader, archive, "future SubD addition", warnings)?,
            0 => {}
            size => reader.skip(usize::from(size))?,
        }
    }
}

fn read_record_end(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<(), SubdError> {
    if archive.value() < 70 {
        expect_zero(reader, "SubD component end marker")
    } else {
        finish_additions(reader, archive, warnings)
    }
}

fn read_pointers(
    reader: &mut BoundedReader<'_>,
    count: usize,
    allow_null: bool,
) -> Result<Vec<ComponentPointer>, SubdError> {
    if count > MAX_INCIDENT_COMPONENTS || count > reader.remaining() / 5 {
        return Err(malformed(
            reader.position(),
            "SubD pointer count exceeds bounded cap",
        ));
    }
    (0..count)
        .map(|_| read_pointer(reader, allow_null))
        .collect()
}

fn read_pointer(
    reader: &mut BoundedReader<'_>,
    allow_null: bool,
) -> Result<ComponentPointer, SubdError> {
    let archive_id = reader.u32()?;
    let flags = reader.u8()?;
    if flags & !0x1 != 0 {
        return Err(malformed(
            reader.position() - 1,
            "SubD component pointer has unknown flag bits",
        ));
    }
    if archive_id == 0 && (!allow_null || flags != 0) {
        return Err(malformed(
            reader.position() - 5,
            "invalid null SubD component pointer",
        ));
    }
    Ok(ComponentPointer {
        archive_id,
        direction: flags & 1 != 0,
    })
}

fn read_untyped_pointer(reader: &mut BoundedReader<'_>) -> Result<(), SubdError> {
    let archive_id = reader.u32()?;
    let flags = reader.u8()?;
    if flags & !0x7 != 0
        || (archive_id == 0 && flags != 0)
        || (archive_id != 0 && !matches!(flags & 0x6, 0x2 | 0x4 | 0x6))
    {
        return Err(malformed(
            reader.position() - 5,
            "invalid SubD symmetry-next pointer",
        ));
    }
    Ok(())
}

fn validate_level(level: &RawLevel, expected_level: usize) -> Result<(), SubdError> {
    let mut types = BTreeMap::new();
    for vertex in &level.vertices {
        types.insert(vertex.base.archive_id, ComponentType::Vertex);
    }
    for edge in &level.edges {
        types.insert(edge.base.archive_id, ComponentType::Edge);
    }
    for face in &level.faces {
        types.insert(face.base.archive_id, ComponentType::Face);
    }
    if types.len()
        != level
            .vertices
            .len()
            .checked_add(level.edges.len())
            .and_then(|value| value.checked_add(level.faces.len()))
            .ok_or_else(|| malformed(0, "SubD map size overflow"))?
    {
        return Err(malformed(0, "duplicate SubD archive ID"));
    }
    for vertex in &level.vertices {
        resolve_all(&types, &vertex.edges, ComponentType::Edge)?;
        resolve_all(&types, &vertex.faces, ComponentType::Face)?;
    }
    for edge in &level.edges {
        resolve_all(&types, &edge.vertices, ComponentType::Vertex)?;
        resolve_all(&types, &edge.faces, ComponentType::Face)?;
        if edge.vertices[0].archive_id == edge.vertices[1].archive_id {
            return Err(malformed(0, "SubD edge has identical endpoints"));
        }
    }
    for face in &level.faces {
        resolve_all(&types, &face.edges, ComponentType::Edge)?;
        if face.edges.len() < 3 {
            return Err(malformed(0, "SubD face has fewer than three edge uses"));
        }
    }

    let vertex_edges = incidence_from_edges(level);
    let vertex_faces = incidence_from_faces(level)?;
    let edge_faces = edge_face_incidence(level)?;
    for vertex in &level.vertices {
        compare_incidence(
            &vertex.edges,
            vertex_edges.get(&vertex.base.archive_id),
            "vertex-edge",
        )?;
        compare_incidence(
            &vertex.faces,
            vertex_faces.get(&vertex.base.archive_id),
            "vertex-face",
        )?;
    }
    for edge in &level.edges {
        compare_incidence(
            &edge.faces,
            edge_faces.get(&edge.base.archive_id),
            "edge-face",
        )?;
    }
    if expected_level == 0 {
        if level.vertices.iter().any(|vertex| vertex.tag == 0) {
            return Err(malformed(0, "level-zero SubD vertex has unset tag"));
        }
        if level.edges.iter().any(|edge| edge.tag == 0) {
            return Err(malformed(0, "level-zero SubD edge has unset tag"));
        }
    }
    Ok(())
}

fn incidence_from_edges(level: &RawLevel) -> BTreeMap<u32, BTreeSet<u32>> {
    let mut result = BTreeMap::<u32, BTreeSet<u32>>::new();
    for edge in &level.edges {
        for vertex in edge.vertices {
            result
                .entry(vertex.archive_id)
                .or_default()
                .insert(edge.base.archive_id);
        }
    }
    result
}

fn incidence_from_faces(level: &RawLevel) -> Result<BTreeMap<u32, BTreeSet<u32>>, SubdError> {
    let edges = level
        .edges
        .iter()
        .map(|edge| (edge.base.archive_id, edge))
        .collect::<BTreeMap<_, _>>();
    let mut result = BTreeMap::<u32, BTreeSet<u32>>::new();
    for face in &level.faces {
        let mut first = None;
        let mut previous_end = None;
        for edge_use in &face.edges {
            let edge = edges
                .get(&edge_use.archive_id)
                .ok_or_else(|| malformed(0, "face references missing SubD edge"))?;
            let endpoints = [edge.vertices[0].archive_id, edge.vertices[1].archive_id];
            let (start, end) = if edge_use.direction {
                (endpoints[1], endpoints[0])
            } else {
                (endpoints[0], endpoints[1])
            };
            if previous_end.is_some_and(|value| value != start) {
                return Err(malformed(0, "SubD face ring is not endpoint-continuous"));
            }
            first.get_or_insert(start);
            previous_end = Some(end);
            result
                .entry(start)
                .or_default()
                .insert(face.base.archive_id);
            result.entry(end).or_default().insert(face.base.archive_id);
        }
        if first != previous_end {
            return Err(malformed(0, "SubD face ring is not closed"));
        }
    }
    Ok(result)
}

fn edge_face_incidence(level: &RawLevel) -> Result<BTreeMap<u32, BTreeSet<u32>>, SubdError> {
    let mut result = BTreeMap::<u32, BTreeSet<u32>>::new();
    for face in &level.faces {
        for edge in &face.edges {
            if !result
                .entry(edge.archive_id)
                .or_default()
                .insert(face.base.archive_id)
            {
                return Err(malformed(0, "SubD face repeats an edge"));
            }
        }
    }
    Ok(result)
}

fn compare_incidence(
    serialized: &[ComponentPointer],
    derived: Option<&BTreeSet<u32>>,
    label: &str,
) -> Result<(), SubdError> {
    let serialized = serialized
        .iter()
        .map(|pointer| pointer.archive_id)
        .collect::<BTreeSet<_>>();
    let empty = BTreeSet::new();
    if &serialized != derived.unwrap_or(&empty) {
        return Err(malformed(
            0,
            format!("SubD {label} incidence is not reciprocal"),
        ));
    }
    Ok(())
}

fn resolve_all(
    types: &BTreeMap<u32, ComponentType>,
    pointers: &[ComponentPointer],
    expected: ComponentType,
) -> Result<(), SubdError> {
    for pointer in pointers {
        if types.get(&pointer.archive_id) != Some(&expected) {
            return Err(malformed(
                0,
                "SubD component pointer does not resolve within its partition",
            ));
        }
    }
    Ok(())
}

fn materialize(
    level: RawLevel,
    scale: f64,
    id: cadmpeg_ir::ids::SubdId,
) -> Result<SubdSurface, SubdError> {
    let vertex_indices = level
        .vertices
        .iter()
        .enumerate()
        .map(|(index, vertex)| {
            u32::try_from(index)
                .map(|index| (vertex.base.archive_id, index))
                .map_err(|_| malformed(0, "SubD vertex index overflow"))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let edge_indices = level
        .edges
        .iter()
        .enumerate()
        .map(|(index, edge)| {
            u32::try_from(index)
                .map(|index| (edge.base.archive_id, index))
                .map_err(|_| malformed(0, "SubD edge index overflow"))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let vertices = level
        .vertices
        .into_iter()
        .map(|vertex| {
            let tag = match vertex.tag {
                1 => SubdVertexTag::Smooth,
                2 => SubdVertexTag::Crease,
                3 => SubdVertexTag::Corner,
                4 => SubdVertexTag::Dart,
                _ => return Err(malformed(0, "invalid materialized SubD vertex tag")),
            };
            Ok(SubdVertex {
                point: Point3::new(
                    crate::wire::scaled_coordinate(vertex.point.x, scale)
                        .ok_or_else(|| malformed(0, "scaled SubD vertex is invalid"))?,
                    crate::wire::scaled_coordinate(vertex.point.y, scale)
                        .ok_or_else(|| malformed(0, "scaled SubD vertex is invalid"))?,
                    crate::wire::scaled_coordinate(vertex.point.z, scale)
                        .ok_or_else(|| malformed(0, "scaled SubD vertex is invalid"))?,
                ),
                tag,
            })
        })
        .collect::<Result<Vec<_>, SubdError>>()?;
    let edges = level
        .edges
        .into_iter()
        .map(|edge| {
            let tag = match edge.tag {
                1 => SubdEdgeTag::Smooth,
                2 => SubdEdgeTag::Crease,
                4 => SubdEdgeTag::SmoothX,
                _ => return Err(malformed(0, "invalid materialized SubD edge tag")),
            };
            Ok(SubdEdge {
                vertices: [
                    *vertex_indices
                        .get(&edge.vertices[0].archive_id)
                        .ok_or_else(|| malformed(0, "missing SubD edge endpoint"))?,
                    *vertex_indices
                        .get(&edge.vertices[1].archive_id)
                        .ok_or_else(|| malformed(0, "missing SubD edge endpoint"))?,
                ],
                sharpness: edge.sharpness,
                tag,
                sector_coefficients: edge.sector_coefficients,
            })
        })
        .collect::<Result<Vec<_>, SubdError>>()?;
    let faces = level
        .faces
        .into_iter()
        .map(|face| {
            Ok(SubdFace {
                edges: face
                    .edges
                    .into_iter()
                    .map(|edge| {
                        Ok(SubdEdgeUse {
                            edge: *edge_indices
                                .get(&edge.archive_id)
                                .ok_or_else(|| malformed(0, "missing SubD face edge"))?,
                            reversed: edge.direction,
                        })
                    })
                    .collect::<Result<Vec<_>, SubdError>>()?,
            })
        })
        .collect::<Result<Vec<_>, SubdError>>()?;
    Ok(SubdSurface {
        id,
        scheme: SubdScheme::CatmullClark,
        vertices,
        edges,
        faces,
        source_object: None,
    })
}

fn validate_partitions(partitions: [u32; 4], offset: usize) -> Result<(), SubdError> {
    if partitions[0] != 1
        || partitions[0] > partitions[1]
        || partitions[1] > partitions[2]
        || partitions[2] > partitions[3]
    {
        return Err(malformed(
            offset,
            "SubD archive-ID partitions are not contiguous and one-based",
        ));
    }
    let total = usize::try_from(partitions[3] - 1)
        .map_err(|_| malformed(offset, "SubD partition size overflow"))?;
    if total > MAX_COMPONENTS_PER_LEVEL {
        return Err(malformed(offset, "SubD partition exceeds component cap"));
    }
    Ok(())
}

fn partition_count(start: u32, end: u32) -> Result<usize, SubdError> {
    usize::try_from(
        end.checked_sub(start)
            .ok_or_else(|| malformed(0, "SubD partition underflow"))?,
    )
    .map_err(|_| malformed(0, "SubD partition conversion overflow"))
}

fn capped_u32(reader: &mut BoundedReader<'_>, cap: usize, label: &str) -> Result<usize, SubdError> {
    let offset = reader.position();
    let value = usize::try_from(reader.u32()?)
        .map_err(|_| malformed(offset, format!("{label} conversion overflow")))?;
    if value > cap {
        return Err(malformed(offset, format!("{label} exceeds cap")));
    }
    Ok(value)
}

fn point(reader: &mut BoundedReader<'_>, label: &str) -> Result<Point3, SubdError> {
    let values = [reader.f64()?, reader.f64()?, reader.f64()?];
    if values.iter().any(|value| !value.is_finite()) {
        return Err(malformed(
            reader.position() - 24,
            format!("{label} is not finite"),
        ));
    }
    Ok(Point3::new(values[0], values[1], values[2]))
}

fn read_finite_values(
    reader: &mut BoundedReader<'_>,
    count: usize,
    label: &str,
) -> Result<(), SubdError> {
    for _ in 0..count {
        if !reader.f64()?.is_finite() {
            return Err(malformed(
                reader.position() - 8,
                format!("{label} is not finite"),
            ));
        }
    }
    Ok(())
}

fn validate_sharpness(value: f64, offset: usize) -> Result<(), SubdError> {
    if !value.is_finite() || value < 0.0 {
        Err(malformed(offset, "SubD edge sharpness is invalid"))
    } else {
        Ok(())
    }
}

fn read_mapping_tag(
    parent: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<(), SubdError> {
    let chunk = anonymous_chunk(parent, archive, "SubD texture mapping tag")?;
    let mut reader = BoundedReader::new(parent.backing_bytes(), chunk.body.start, chunk.body.end)?;
    let major = reader.i32()?;
    let minor = reader.i32()?;
    if major != 1 || !(0..=1).contains(&minor) {
        return Err(SubdError::UnsupportedVersion {
            offset: chunk.body.start,
            message: format!("unsupported SubD mapping-tag version {major}.{minor}"),
        });
    }
    reader.take(16)?;
    reader.i32()?;
    read_finite_values(&mut reader, 16, "SubD mapping transform")?;
    if minor >= 1 {
        reader.u32()?;
    }
    finish_chunk(parent, &chunk, reader, warnings)
}

fn read_symmetry(
    parent: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<(), SubdError> {
    let chunk = anonymous_chunk(parent, archive, "SubD symmetry")?;
    let mut reader = BoundedReader::new(parent.backing_bytes(), chunk.body.start, chunk.body.end)?;
    let major = reader.i32()?;
    let version = reader.i32()?;
    if major != 1 || !(1..=4).contains(&version) {
        return Err(SubdError::UnsupportedVersion {
            offset: chunk.body.start,
            message: format!("unsupported SubD symmetry version {major}.{version}"),
        });
    }
    let mut symmetry_type = reader.u8()?;
    if symmetry_type == 113 {
        symmetry_type = 2;
    }
    if symmetry_type == 0 {
        return finish_chunk(parent, &chunk, reader, warnings);
    }
    if !(1..=5).contains(&symmetry_type) {
        return Err(malformed(
            reader.position() - 1,
            "invalid SubD symmetry type",
        ));
    }
    reader.u32()?;
    reader.u32()?;
    reader.take(16)?;
    let inner = anonymous_chunk(&reader, archive, "SubD symmetry transform")?;
    let mut transform =
        BoundedReader::new(reader.backing_bytes(), inner.body.start, inner.body.end)?;
    let inner_major = transform.i32()?;
    let inner_version = transform.i32()?;
    if inner_major != 1 || inner_version < 0 {
        return Err(SubdError::UnsupportedVersion {
            offset: inner.body.start,
            message: format!(
                "unsupported SubD symmetry transform version {inner_major}.{inner_version}"
            ),
        });
    }
    match symmetry_type {
        1 => read_finite_values(&mut transform, 4, "SubD reflection plane")?,
        2 => {
            read_finite_values(&mut transform, 6, "SubD rotation axis")?;
            if inner_version >= 2 {
                read_finite_values(&mut transform, 4, "SubD rotation plane")?;
            }
        }
        3 => {
            read_finite_values(&mut transform, 4, "SubD reflection plane")?;
            read_finite_values(&mut transform, 6, "SubD rotation axis")?;
        }
        4 | 5 => {
            read_finite_values(&mut transform, 16, "SubD symmetry transform")?;
            if inner_version >= 2 {
                read_finite_values(&mut transform, 4, "SubD symmetry plane")?;
            }
        }
        _ => unreachable!("symmetry type checked"),
    }
    finish_chunk(&mut reader, &inner, transform, warnings)?;
    if version >= 2 && reader.u8()? > 2 {
        return Err(malformed(
            reader.position() - 1,
            "invalid SubD symmetry coordinate system",
        ));
    }
    if version >= 3 {
        reader.u64()?;
    }
    if version >= 4 {
        read_sha1(&mut reader, archive, warnings)?;
        read_sha1(&mut reader, archive, warnings)?;
    }
    finish_chunk(parent, &chunk, reader, warnings)
}

fn read_subd_hash(
    parent: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<(), SubdError> {
    let chunk = anonymous_chunk(parent, archive, "SubD topology hash")?;
    let mut reader = BoundedReader::new(parent.backing_bytes(), chunk.body.start, chunk.body.end)?;
    let major = reader.i32()?;
    let minor = reader.i32()?;
    if major != 1 || minor != 1 {
        return Err(SubdError::UnsupportedVersion {
            offset: chunk.body.start,
            message: format!("unsupported SubD topology-hash version {major}.{minor}"),
        });
    }
    if !reader.bool()? {
        reader.u8()?;
        reader.u32()?;
        read_sha1(&mut reader, archive, warnings)?;
        reader.u32()?;
        read_sha1(&mut reader, archive, warnings)?;
        reader.u32()?;
        read_sha1(&mut reader, archive, warnings)?;
    }
    finish_chunk(parent, &chunk, reader, warnings)
}

fn read_sha1(
    parent: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    warnings: &mut Vec<String>,
) -> Result<(), SubdError> {
    let chunk = anonymous_chunk(parent, archive, "SHA-1 hash")?;
    let mut reader = BoundedReader::new(parent.backing_bytes(), chunk.body.start, chunk.body.end)?;
    let major = reader.i32()?;
    let minor = reader.i32()?;
    if major != 1 || minor != 0 {
        return Err(SubdError::UnsupportedVersion {
            offset: chunk.body.start,
            message: format!("unsupported SHA-1 record version {major}.{minor}"),
        });
    }
    reader.take(20)?;
    finish_chunk(parent, &chunk, reader, warnings)
}

fn expect_zero(reader: &mut BoundedReader<'_>, label: &str) -> Result<(), SubdError> {
    if reader.u8()? == 0 {
        Ok(())
    } else {
        Err(malformed(
            reader.position() - 1,
            format!("{label} is not zero"),
        ))
    }
}

fn anonymous_chunk(
    reader: &BoundedReader<'_>,
    archive: ArchiveVersion,
    label: &str,
) -> Result<crate::chunks::Chunk, SubdError> {
    let chunk = chunk_at(
        reader.backing_bytes(),
        reader.position(),
        reader.end(),
        archive,
        false,
    )?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(malformed(
            chunk.header_start,
            format!("expected bounded anonymous {label} chunk"),
        ));
    }
    Ok(chunk)
}

fn consume_anonymous(
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    label: &str,
    warnings: &mut Vec<String>,
) -> Result<(), SubdError> {
    let chunk = anonymous_chunk(reader, archive, label)?;
    if matches!(
        verify_checksum(reader.backing_bytes(), &chunk)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push(format!(
            "{label} CRC mismatch at offset {}",
            chunk.header_start
        ));
    }
    reader.skip(chunk.next_offset - reader.position())?;
    Ok(())
}

fn finish_chunk(
    parent: &mut BoundedReader<'_>,
    chunk: &crate::chunks::Chunk,
    child: BoundedReader<'_>,
    warnings: &mut Vec<String>,
) -> Result<(), SubdError> {
    if child.remaining() != 0 {
        return Err(malformed(
            child.position(),
            "SubD anonymous chunk has trailing bytes",
        ));
    }
    if matches!(
        verify_checksum(parent.backing_bytes(), chunk)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        warnings.push(format!(
            "SubD anonymous CRC mismatch at offset {}",
            chunk.header_start
        ));
    }
    parent.skip(chunk.next_offset - parent.position())?;
    Ok(())
}

fn finish_payload(reader: &BoundedReader<'_>) -> Result<(), SubdError> {
    if reader.remaining() == 0 {
        Ok(())
    } else {
        Err(malformed(
            reader.position(),
            "ON_SubD payload has trailing bytes",
        ))
    }
}

fn malformed(offset: usize, message: impl Into<String>) -> SubdError {
    SubdError::Malformed {
        offset,
        message: message.into(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    #[expect(
        clippy::struct_excessive_bools,
        reason = "independent fixture toggles model orthogonal archive mutations"
    )]
    struct Fixture {
        archive: ArchiveVersion,
        minor: i32,
        reversed_edge: bool,
        open_ring: bool,
        bad_pointer_type: bool,
        null_endpoint: bool,
        omit_vertex_edge: bool,
        vertex_tag: u8,
        edge_tag: u8,
        end_sharpness: f64,
        level_count: usize,
        render_mesh: bool,
        future_additions: bool,
        saved_limit_points: bool,
    }

    impl Default for Fixture {
        fn default() -> Self {
            Self {
                archive: ArchiveVersion::V5,
                minor: 0,
                reversed_edge: false,
                open_ring: false,
                bad_pointer_type: false,
                null_endpoint: false,
                omit_vertex_edge: false,
                vertex_tag: 1,
                edge_tag: 1,
                end_sharpness: 0.25,
                level_count: 1,
                render_mesh: false,
                future_additions: false,
                saved_limit_points: false,
            }
        }
    }

    fn anonymous(body: &[u8]) -> Vec<u8> {
        let mut bytes = ANONYMOUS.to_le_bytes().to_vec();
        bytes.extend_from_slice(
            &i64::try_from(body.len() + 4)
                .expect("anonymous length")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(&crc32fast::hash(body).to_le_bytes());
        bytes
    }

    fn pointer(bytes: &mut Vec<u8>, id: u32, flags: u8) {
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.push(flags);
    }

    fn base(bytes: &mut Vec<u8>, fixture: Fixture, archive_id: u32, level: u16) {
        bytes.extend_from_slice(&archive_id.to_le_bytes());
        bytes.extend_from_slice(&(archive_id + 100).to_le_bytes());
        bytes.extend_from_slice(&level.to_le_bytes());
        if fixture.archive.value() < 70 {
            bytes.extend([0, 0]);
        } else {
            bytes.extend([0, 0, 0]);
            if fixture.future_additions {
                bytes.push(254);
                bytes.extend(anonymous(&[1, 0, 0, 0]));
                bytes.push(3);
                bytes.extend([7, 8, 9]);
            }
            bytes.push(255);
        }
    }

    fn record_end(bytes: &mut Vec<u8>, fixture: Fixture) {
        bytes.push(if fixture.archive.value() < 70 { 0 } else { 255 });
    }

    fn vertex(
        bytes: &mut Vec<u8>,
        fixture: Fixture,
        archive_id: u32,
        point_value: [f64; 3],
        edges: &[u32],
        level: u16,
    ) {
        base(bytes, fixture, archive_id, level);
        bytes.push(fixture.vertex_tag);
        for value in point_value {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let serialized_edges = if fixture.omit_vertex_edge && archive_id == 1 {
            &edges[1..]
        } else {
            edges
        };
        bytes.extend_from_slice(
            &u16::try_from(serialized_edges.len())
                .expect("edge count")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        if fixture.saved_limit_points && archive_id == 1 {
            bytes.push(4);
            bytes.extend_from_slice(&1_u32.to_le_bytes());
            for value in 0..12 {
                bytes.extend_from_slice(&(f64::from(value)).to_le_bytes());
            }
            pointer(bytes, 9, 0);
        } else {
            bytes.push(0);
        }
        bytes.extend_from_slice(
            &u16::try_from(serialized_edges.len())
                .expect("edge count")
                .to_le_bytes(),
        );
        for edge in serialized_edges {
            pointer(bytes, *edge, 0);
        }
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        pointer(bytes, 9, 0);
        record_end(bytes, fixture);
    }

    fn edge(
        bytes: &mut Vec<u8>,
        fixture: Fixture,
        archive_id: u32,
        endpoints: [u32; 2],
        level: u16,
    ) {
        base(bytes, fixture, archive_id, level);
        bytes.push(fixture.edge_tag);
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&0.125_f64.to_le_bytes());
        bytes.extend_from_slice(&0.875_f64.to_le_bytes());
        bytes.extend_from_slice(&0.25_f64.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        let first = if fixture.null_endpoint && archive_id == 5 {
            0
        } else {
            endpoints[0]
        };
        pointer(bytes, first, 0);
        pointer(
            bytes,
            endpoints[1],
            if fixture.bad_pointer_type && archive_id == 5 {
                0x2
            } else {
                0
            },
        );
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        pointer(bytes, 9, 0);
        if fixture.archive.value() < 70 {
            bytes.push(0);
        } else {
            if fixture.archive.value() >= 80 {
                bytes.push(8);
                bytes.extend_from_slice(&fixture.end_sharpness.to_le_bytes());
            }
            bytes.push(255);
        }
    }

    fn face(bytes: &mut Vec<u8>, fixture: Fixture, level: u16) {
        base(bytes, fixture, 9, level);
        bytes.extend_from_slice(&9_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        pointer(bytes, 5, 0);
        pointer(bytes, 6, u8::from(fixture.reversed_edge));
        pointer(bytes, 7, 0);
        pointer(bytes, if fixture.open_ring { 5 } else { 8 }, 0);
        record_end(bytes, fixture);
    }

    fn level(fixture: Fixture, level: u16) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&1_i32.to_le_bytes());
        body.extend_from_slice(&1_i32.to_le_bytes());
        body.extend_from_slice(&level.to_le_bytes());
        body.extend([4, 4, 4]);
        for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 0.0] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        for partition in [1_u32, 5, 9, 10] {
            body.extend_from_slice(&partition.to_le_bytes());
        }
        vertex(&mut body, fixture, 1, [0.0, 0.0, 0.0], &[5, 8], level);
        vertex(&mut body, fixture, 2, [1.0, 0.0, 0.0], &[5, 6], level);
        vertex(&mut body, fixture, 3, [1.0, 1.0, 0.0], &[6, 7], level);
        vertex(&mut body, fixture, 4, [0.0, 1.0, 0.0], &[7, 8], level);
        edge(&mut body, fixture, 5, [1, 2], level);
        edge(
            &mut body,
            fixture,
            6,
            if fixture.reversed_edge {
                [3, 2]
            } else {
                [2, 3]
            },
            level,
        );
        edge(&mut body, fixture, 7, [3, 4], level);
        edge(&mut body, fixture, 8, [4, 1], level);
        face(&mut body, fixture, level);
        body.push(u8::from(fixture.render_mesh));
        if fixture.render_mesh {
            body.extend(anonymous(&[1, 0, 0, 0]));
        }
        anonymous(&body)
    }

    fn payload(fixture: Fixture) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&1_i32.to_le_bytes());
        body.extend_from_slice(&fixture.minor.to_le_bytes());
        body.extend_from_slice(
            &u32::try_from(fixture.level_count)
                .expect("level count")
                .to_le_bytes(),
        );
        body.extend_from_slice(&9_u32.to_le_bytes());
        body.extend_from_slice(&9_u32.to_le_bytes());
        body.extend_from_slice(&9_u32.to_le_bytes());
        for value in [0.0_f64, 0.0, 0.0, 1.0, 1.0, 0.0] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        for level_index in 0..fixture.level_count {
            body.extend(level(
                fixture,
                u16::try_from(level_index).expect("level index"),
            ));
        }
        if fixture.minor >= 1 {
            body.push(0);
            let mut mapping = Vec::new();
            mapping.extend_from_slice(&1_i32.to_le_bytes());
            mapping.extend_from_slice(&0_i32.to_le_bytes());
            mapping.extend([0; 16]);
            mapping.extend_from_slice(&0_i32.to_le_bytes());
            for index in 0..16 {
                mapping
                    .extend_from_slice(&(if index % 5 == 0 { 1.0_f64 } else { 0.0 }).to_le_bytes());
            }
            body.extend(anonymous(&mapping));
        }
        if fixture.minor >= 2 {
            let mut symmetry = Vec::new();
            symmetry.extend_from_slice(&1_i32.to_le_bytes());
            symmetry.extend_from_slice(&4_i32.to_le_bytes());
            symmetry.push(0);
            body.extend(anonymous(&symmetry));
        }
        if fixture.minor >= 3 {
            body.extend_from_slice(&42_u64.to_le_bytes());
        }
        if fixture.minor >= 4 {
            body.push(0);
            body.extend([0; 16]);
            body.push(0);
            let mut hash = Vec::new();
            hash.extend_from_slice(&1_i32.to_le_bytes());
            hash.extend_from_slice(&1_i32.to_le_bytes());
            hash.push(1);
            body.extend(anonymous(&hash));
        }
        let mut payload = vec![1];
        payload.extend(anonymous(&body));
        payload
    }

    pub(crate) fn quad_payload(archive: ArchiveVersion) -> Vec<u8> {
        payload(Fixture {
            archive,
            ..Fixture::default()
        })
    }

    fn decode_fixture(fixture: Fixture, scale: f64) -> Result<DecodedSubd, SubdError> {
        let bytes = payload(fixture);
        decode(
            &bytes,
            0..bytes.len(),
            fixture.archive,
            scale,
            "test:subd#0".into(),
        )
    }

    #[test]
    fn decodes_empty_outer_subd_without_carrier() {
        assert!(matches!(
            decode(&[0], 0..1, ArchiveVersion::V5, 1.0, "test:subd#0".into())
                .expect("required invariant"),
            DecodedSubd::Empty
        ));
        assert!(decode(&[2], 0..1, ArchiveVersion::V5, 1.0, "test:subd#0".into()).is_err());
    }

    #[test]
    fn nested_crc_mismatch_warns_without_discarding_subd() {
        let fixture = Fixture::default();
        let mut bytes = payload(fixture);
        let crc = bytes.len() - 1;
        bytes[crc] ^= 1;
        let decoded = decode(
            &bytes,
            0..bytes.len(),
            fixture.archive,
            1.0,
            "test:subd#0".into(),
        )
        .expect("recoverable checksum mismatch");
        let DecodedSubd::Surface { warnings, .. } = decoded else {
            panic!("expected surface");
        };
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("SubD anonymous CRC mismatch"));
    }

    #[test]
    fn decodes_minor_suffix_gates_across_archive_bands() {
        for (minor, archive) in [
            (0, ArchiveVersion::V5),
            (1, ArchiveVersion::V6),
            (2, ArchiveVersion::V7),
            (3, ArchiveVersion::V7),
            (4, ArchiveVersion::V8),
        ] {
            let decoded = decode_fixture(
                Fixture {
                    archive,
                    minor,
                    ..Fixture::default()
                },
                1.0,
            )
            .expect("required invariant");
            assert!(matches!(decoded, DecodedSubd::Surface { .. }));
        }
    }

    #[test]
    fn decodes_valid_old_and_new_component_bases() {
        for archive in [
            ArchiveVersion::V5,
            ArchiveVersion::V6,
            ArchiveVersion::V7,
            ArchiveVersion::V8,
        ] {
            assert!(decode_fixture(
                Fixture {
                    archive,
                    ..Fixture::default()
                },
                1.0
            )
            .is_ok());
        }
    }

    #[test]
    fn preserves_directed_reversed_face_edge_use() {
        let DecodedSubd::Surface { surface, .. } = decode_fixture(
            Fixture {
                reversed_edge: true,
                ..Fixture::default()
            },
            1.0,
        )
        .expect("required invariant") else {
            panic!("expected surface");
        };
        assert!(surface.faces[0].edges[1].reversed);
        assert_eq!(surface.edges[1].vertices, [2, 1]);
    }

    #[test]
    fn rejects_open_or_repeated_face_rings() {
        assert!(decode_fixture(
            Fixture {
                open_ring: true,
                ..Fixture::default()
            },
            1.0
        )
        .is_err());
    }

    #[test]
    fn rejects_pointer_type_null_and_reciprocity_errors() {
        for fixture in [
            Fixture {
                bad_pointer_type: true,
                ..Fixture::default()
            },
            Fixture {
                null_endpoint: true,
                ..Fixture::default()
            },
            Fixture {
                omit_vertex_edge: true,
                ..Fixture::default()
            },
        ] {
            assert!(decode_fixture(fixture, 1.0).is_err());
        }
    }

    #[test]
    fn preserves_vertex_edge_tags_and_sector_coefficients() {
        let DecodedSubd::Surface { surface, .. } = decode_fixture(
            Fixture {
                vertex_tag: 4,
                edge_tag: 4,
                ..Fixture::default()
            },
            1.0,
        )
        .expect("required invariant") else {
            panic!("expected surface");
        };
        assert_eq!(surface.vertices[0].tag, SubdVertexTag::Dart);
        assert_eq!(surface.edges[0].tag, SubdEdgeTag::SmoothX);
        assert_eq!(surface.edges[0].sector_coefficients, [0.125, 0.875]);
    }

    #[test]
    fn maps_scalar_and_preserves_v8_two_ended_sharpness() {
        let DecodedSubd::Surface { surface, .. } =
            decode_fixture(Fixture::default(), 1.0).expect("required invariant")
        else {
            panic!("expected old surface");
        };
        assert_eq!(surface.edges[0].sharpness, [0.25, 0.25]);
        let DecodedSubd::Surface { surface, .. } = decode_fixture(
            Fixture {
                archive: ArchiveVersion::V8,
                end_sharpness: 0.75,
                ..Fixture::default()
            },
            1.0,
        )
        .expect("required invariant") else {
            panic!("expected V8 surface");
        };
        assert_eq!(surface.edges[0].sharpness, [0.25, 0.75]);
    }

    #[test]
    fn consumes_saved_limit_points_and_future_additions() {
        assert!(decode_fixture(
            Fixture {
                archive: ArchiveVersion::V7,
                saved_limit_points: true,
                future_additions: true,
                ..Fixture::default()
            },
            1.0
        )
        .is_ok());
    }

    #[test]
    fn validates_higher_levels_and_render_mesh_chunks() {
        let decoded = decode_fixture(
            Fixture {
                archive: ArchiveVersion::V8,
                level_count: 2,
                render_mesh: true,
                ..Fixture::default()
            },
            1.0,
        )
        .expect("required invariant");
        let DecodedSubd::Surface {
            neutral_metadata, ..
        } = decoded
        else {
            panic!("expected surface");
        };
        assert!(neutral_metadata);
    }

    #[test]
    fn scales_control_points_once_without_scaling_edge_metadata() {
        let DecodedSubd::Surface { surface, .. } =
            decode_fixture(Fixture::default(), 25.4).expect("required invariant")
        else {
            panic!("expected surface");
        };
        assert_eq!(surface.vertices[2].point, Point3::new(25.4, 25.4, 0.0));
        assert_eq!(surface.edges[0].sharpness, [0.25, 0.25]);
        assert_eq!(surface.edges[0].sector_coefficients, [0.125, 0.875]);
    }

    #[test]
    fn rejects_noncontiguous_partitions_and_future_versions() {
        let mut bytes = payload(Fixture::default());
        let subd_chunk_header = 1 + 12;
        let subd_version_and_header = 8 + 4 + 12 + 48;
        let level_chunk_header = 12;
        let level_partition_offset =
            subd_chunk_header + subd_version_and_header + level_chunk_header + 8 + 2 + 3 + 48;
        bytes[level_partition_offset..level_partition_offset + 4]
            .copy_from_slice(&2_u32.to_le_bytes());
        assert!(decode(
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            1.0,
            "test:subd#0".into()
        )
        .is_err());

        let mut future = payload(Fixture::default());
        future[(1 + 12)..=16].copy_from_slice(&2_i32.to_le_bytes());
        assert!(matches!(
            decode(
                &future,
                0..future.len(),
                ArchiveVersion::V5,
                1.0,
                "test:subd#0".into()
            ),
            Err(SubdError::UnsupportedVersion { .. })
        ));
    }
}
