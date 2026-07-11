// SPDX-License-Identifier: Apache-2.0
//! Isolated `ON_Brep` parsing and semantic validation.
//!
//! This module deliberately stops at a validated native representation.  No
//! topology IDs or IR carriers are created here.

use std::ops::Range;

use crate::chunks::{
    chunk_at, verify_checksum, ArchiveVersion, BoundedReader, ChecksumStatus, Chunk,
};
use crate::curves::{error, unsupported, GeometryError};
use crate::objects::{parse_class_wrapper, Uuid};
use crate::settings::{bbox, interval, BoundingBox, Interval, Point3};

/// `ON_Brep` class UUID.
pub(crate) const ON_BREP: &str = "60b5dbc5-e660-11d3-bfe4-0010830122f0";
/// Maximum number of records in one Brep array.
pub(crate) const MAX_BREP_ITEMS: usize = 1 << 20;
/// Maximum nesting depth used while reading polymorphic children.
pub(crate) const MAX_BREP_DEPTH: usize = 32;
const ANONYMOUS: u32 = 0x4000_8000;
const ON_UNSET_VALUE: f64 = 1.234_321_012_343_21e308;
const ON_UNSET_NEGATIVE_VALUE: f64 = -ON_UNSET_VALUE;

/// The base class family expected by a polymorphic Brep slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawBrepBaseType {
    /// A curve-derived OpenNURBS class.
    Curve,
    /// A surface-derived OpenNURBS class.
    Surface,
    /// A class outside the expected family.
    Other,
}

/// A polymorphic Brep child slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawBrepChild {
    /// Class UUID, when the slot is present.
    pub(crate) class_uuid: Uuid,
    /// Class-data byte range.
    pub(crate) class_data_range: Range<usize>,
    /// Complete class-wrapper byte range.
    pub(crate) source_range: Range<usize>,
    /// Base-class family inferred from the class UUID.
    pub(crate) base_type: RawBrepBaseType,
}

/// A positional polymorphic Brep array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawBrepChildren {
    /// Child slots, including null slots.
    pub(crate) slots: Vec<Option<RawBrepChild>>,
    /// Anonymous wrapper byte range.
    pub(crate) source_range: Range<usize>,
    /// Base-class family required by this array.
    pub(crate) expected_type: RawBrepBaseType,
}

/// A raw Brep vertex.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepVertex {
    /// Positional record index.
    pub(crate) index: i32,
    /// Vertex point.
    pub(crate) point: Point3,
    /// Incident edge indexes.
    pub(crate) edges: Vec<i32>,
    /// Vertex tolerance.
    pub(crate) tolerance: f64,
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// A raw Brep edge.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepEdge {
    /// Positional record index.
    pub(crate) index: i32,
    /// C3 curve slot.
    pub(crate) curve: i32,
    /// Proxy reversal flag.
    pub(crate) proxy_reversed: i32,
    /// Proxy domain.
    pub(crate) proxy_domain: Interval,
    /// Endpoint vertex indexes.
    pub(crate) vertices: [i32; 2],
    /// Incident trim indexes.
    pub(crate) trims: Vec<i32>,
    /// Edge tolerance.
    pub(crate) tolerance: f64,
    /// Native edge domain.
    pub(crate) domain: Interval,
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// A raw Brep trim.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepTrim {
    /// Positional record index.
    pub(crate) index: i32,
    /// C2 curve slot.
    pub(crate) curve: i32,
    /// Proxy domain.
    pub(crate) proxy_domain: Interval,
    /// Edge index, or `-1` for singular and point trims.
    pub(crate) edge: i32,
    /// Start and end vertex indexes.
    pub(crate) vertices: [i32; 2],
    /// Three-dimensional reversal flag.
    pub(crate) reversed_3d: i32,
    /// Raw trim-type value.
    pub(crate) trim_type: i32,
    /// Raw ISO value.
    pub(crate) iso: i32,
    /// Loop index.
    pub(crate) loop_index: i32,
    /// Two-dimensional and three-dimensional tolerances.
    pub(crate) tolerances: [f64; 2],
    /// Native trim domain.
    pub(crate) domain: Interval,
    /// Proxy reversal byte.
    pub(crate) proxy_reversed: u8,
    /// Reserved bytes from the current layout.
    pub(crate) reserved: Vec<u8>,
    /// Legacy 2D and 3D tolerances appended after the proxy block.
    pub(crate) legacy_tolerances: [f64; 2],
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// A raw Brep loop.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepLoop {
    /// Positional record index.
    pub(crate) index: i32,
    /// Directed trim ring.
    pub(crate) trims: Vec<i32>,
    /// Raw loop-type value.
    pub(crate) loop_type: i32,
    /// Face index.
    pub(crate) face: i32,
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// A raw Brep face.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepFace {
    /// Positional record index.
    pub(crate) index: i32,
    /// Face loop indexes.
    pub(crate) loops: Vec<i32>,
    /// Surface slot.
    pub(crate) surface: i32,
    /// Surface reversal flag.
    pub(crate) reversed_surface: i32,
    /// Material channel.
    pub(crate) material_channel: i32,
    /// Optional face UUID.
    pub(crate) uuid: Option<Uuid>,
    /// Optional per-face color.
    pub(crate) color: Option<[u8; 4]>,
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// A render or analysis mesh cache slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawBrepMeshSlot {
    /// Present mesh child, if it passed class validation.
    pub(crate) mesh: Option<RawBrepChild>,
    /// Whether the archive supplied a nonzero presence byte.
    pub(crate) present: bool,
}

/// A raw region face side.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepFaceSide {
    /// Positional side index.
    pub(crate) index: i32,
    /// Region index, or `-1` when unassigned.
    pub(crate) region: i32,
    /// Face index.
    pub(crate) face: i32,
    /// Surface-normal direction.
    pub(crate) direction: i32,
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// A raw Brep region.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrepRegion {
    /// Positional region index.
    pub(crate) index: i32,
    /// Raw region type.
    pub(crate) region_type: i32,
    /// Member face-side indexes.
    pub(crate) sides: Vec<i32>,
    /// Region bounds.
    pub(crate) bounds: BoundingBox,
    /// Complete record byte range.
    pub(crate) source_range: Range<usize>,
}

/// Parsed Brep data before semantic validation.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RawBrep {
    /// Packed payload minor.
    pub(crate) minor: u8,
    /// C2 curve slots.
    pub(crate) c2: RawBrepChildren,
    /// C3 curve slots.
    pub(crate) c3: RawBrepChildren,
    /// Surface slots.
    pub(crate) surfaces: RawBrepChildren,
    /// Vertex records.
    pub(crate) vertices: Vec<RawBrepVertex>,
    /// Edge records.
    pub(crate) edges: Vec<RawBrepEdge>,
    /// Trim records.
    pub(crate) trims: Vec<RawBrepTrim>,
    /// Loop records.
    pub(crate) loops: Vec<RawBrepLoop>,
    /// Face records.
    pub(crate) faces: Vec<RawBrepFace>,
    /// Brep bounds.
    pub(crate) bounds: BoundingBox,
    /// Render mesh cache slots.
    pub(crate) render_meshes: Vec<RawBrepMeshSlot>,
    /// Analysis mesh cache slots.
    pub(crate) analysis_meshes: Vec<RawBrepMeshSlot>,
    /// Raw solid state, normalized only by validation.
    pub(crate) is_solid: Option<i32>,
    /// Region face sides.
    pub(crate) face_sides: Vec<RawBrepFaceSide>,
    /// Regions.
    pub(crate) regions: Vec<RawBrepRegion>,
    /// Complete payload range.
    pub(crate) source_range: Range<usize>,
    /// Complete vertex-array wrapper range.
    pub(crate) vertex_array_range: Range<usize>,
    /// Complete edge-array wrapper range.
    pub(crate) edge_array_range: Range<usize>,
    /// Complete trim-array wrapper range.
    pub(crate) trim_array_range: Range<usize>,
    /// Complete loop-array wrapper range.
    pub(crate) loop_array_range: Range<usize>,
    /// Complete face-array wrapper range.
    pub(crate) face_array_range: Range<usize>,
}

/// A semantically validated raw Brep.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ValidatedRawBrep {
    /// Validated Brep payload.
    pub(crate) raw: RawBrep,
    /// Warnings for discarded optional caches or region topology.
    pub(crate) warnings: Vec<String>,
}

/// Parses and validates one `ON_Brep` class-data payload.
pub(crate) fn parse(
    bytes: &[u8],
    range: Range<usize>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
) -> Result<ValidatedRawBrep, GeometryError> {
    let mut reader = BoundedReader::new(bytes, range.start, range.end)?;
    let version_offset = reader.position();
    let version = reader.u8()?;
    if version >> 4 != 3 {
        return Err(unsupported(version_offset, "unsupported ON_Brep major"));
    }
    if version & 0x0f > 3 {
        return Err(unsupported(version_offset, "unsupported ON_Brep minor"));
    }
    let minor = version & 0x0f;
    let mut warnings = Vec::new();
    let c2 = read_children(bytes, &mut reader, archive, RawBrepBaseType::Curve, 0)?;
    let c3 = read_children(bytes, &mut reader, archive, RawBrepBaseType::Curve, 0)?;
    let surfaces = read_children(bytes, &mut reader, archive, RawBrepBaseType::Surface, 0)?;
    let (vertices, vertex_array_range) = read_vertices(bytes, &mut reader, archive)?;
    let (edges, edge_array_range) = read_edges(bytes, &mut reader, archive, writer_version)?;
    let (trims, trim_array_range) = read_trims(bytes, &mut reader, archive, writer_version)?;
    let (loops, loop_array_range) = read_loops(bytes, &mut reader, archive)?;
    let (faces, face_array_range) = read_faces(bytes, &mut reader, archive)?;
    let bounds = bbox(&mut reader)?;
    let (render_meshes, analysis_meshes) = if minor >= 1 {
        (
            read_mesh_sides(bytes, &mut reader, archive, faces.len(), &mut warnings)?,
            read_mesh_sides(bytes, &mut reader, archive, faces.len(), &mut warnings)?,
        )
    } else {
        (Vec::new(), Vec::new())
    };
    let is_solid = if minor >= 2 {
        let value = reader.i32()?;
        if (0..=3).contains(&value) {
            Some(value)
        } else {
            warnings.push(format!(
                "invalid Brep is_solid value {value}; normalized unset"
            ));
            None
        }
    } else {
        None
    };
    let (face_sides, regions) = if minor >= 3 {
        read_regions(bytes, &mut reader, archive, faces.len(), &mut warnings)?
    } else {
        (Vec::new(), Vec::new())
    };
    if reader.remaining() != 0 {
        return Err(error(
            reader.position(),
            "ON_Brep payload has trailing bytes",
        ));
    }
    let raw = RawBrep {
        minor,
        c2,
        c3,
        surfaces,
        vertices,
        edges,
        trims,
        loops,
        faces,
        bounds,
        render_meshes,
        analysis_meshes,
        is_solid,
        face_sides,
        regions,
        source_range: range,
        vertex_array_range,
        edge_array_range,
        trim_array_range,
        loop_array_range,
        face_array_range,
    };
    let mut validated = validate(raw)?;
    validated.warnings.splice(0..0, warnings);
    Ok(validated)
}

/// Returns whether a UUID is `ON_Brep`.
pub(crate) fn supported_class(uuid: Uuid) -> bool {
    uuid.to_string() == ON_BREP
}

fn read_children(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    expected_type: RawBrepBaseType,
    depth: usize,
) -> Result<RawBrepChildren, GeometryError> {
    if depth > MAX_BREP_DEPTH {
        return Err(error(
            reader.position(),
            "Brep child recursion limit exceeded",
        ));
    }
    let start = reader.position();
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child_reader = body_reader(bytes, &chunk)?;
    let (major, minor) = (child_reader.i32()?, child_reader.i32()?);
    if major != 1 || minor != 0 {
        return Err(unsupported(
            child_reader.position() - 8,
            "unsupported Brep polymorphic-array version",
        ));
    }
    let count = count(&mut child_reader, MAX_BREP_ITEMS)?;
    let mut slots = Vec::with_capacity(count);
    for _ in 0..count {
        let present = child_reader.i32()?;
        match present {
            0 => slots.push(None),
            1 => {
                let child_start = child_reader.position();
                let child_chunk = chunk_at(bytes, child_start, child_reader.end(), archive, false)?;
                let child_end = child_chunk.next_offset;
                let class = parse_class_wrapper(
                    bytes,
                    chunk_start_range(&child_chunk),
                    archive,
                    &mut Vec::new(),
                )?;
                child_reader.skip(child_end - child_start)?;
                let base_type = classify_base_type(class.class_uuid);
                slots.push(Some(RawBrepChild {
                    class_uuid: class.class_uuid,
                    class_data_range: class.class_data_range,
                    source_range: child_start..child_end,
                    base_type,
                }));
            }
            _ => {
                return Err(error(
                    child_reader.position() - 4,
                    "invalid Brep slot presence",
                ))
            }
        }
    }
    finish_anonymous(bytes, reader, &chunk, child_reader)?;
    Ok(RawBrepChildren {
        slots,
        source_range: start..chunk.next_offset,
        expected_type,
    })
}

fn read_vertices(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<(Vec<RawBrepVertex>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let count = raw_array_start(&mut child, "vertex", 40)?;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let start = child.position();
        let index = child.i32()?;
        let point = point(&mut child)?;
        let edges = indexes(&mut child, "vertex edge")?;
        let tolerance = child.f64()?;
        result.push(RawBrepVertex {
            index,
            point,
            edges,
            tolerance,
            source_range: start..child.position(),
        });
    }
    let range = chunk.range();
    finish_anonymous(bytes, reader, &chunk, child)?;
    Ok((result, range))
}

fn read_edges(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
) -> Result<(Vec<RawBrepEdge>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let count = raw_array_start(&mut child, "edge", 44)?;
    let current = archive.value() >= 3 && writer_version.is_some_and(|v| v >= 200_206_180);
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let start = child.position();
        let index = child.i32()?;
        let curve = child.i32()?;
        let proxy_reversed = child.i32()?;
        let proxy_domain = interval(&mut child)?;
        let vertices = [child.i32()?, child.i32()?];
        let trims = indexes(&mut child, "edge trim")?;
        let tolerance = child.f64()?;
        let domain = if current {
            interval(&mut child)?
        } else {
            proxy_domain
        };
        result.push(RawBrepEdge {
            index,
            curve,
            proxy_reversed,
            proxy_domain,
            vertices,
            trims,
            tolerance,
            domain,
            source_range: start..child.position(),
        });
    }
    let range = chunk.range();
    finish_anonymous(bytes, reader, &chunk, child)?;
    Ok((result, range))
}

fn read_trims(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    writer_version: Option<i64>,
) -> Result<(Vec<RawBrepTrim>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let count = raw_array_start(&mut child, "trim", 132)?;
    let current = archive.value() >= 3 && writer_version.is_some_and(|v| v >= 200_206_180);
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let start = child.position();
        let index = child.i32()?;
        let curve = child.i32()?;
        let proxy_domain = interval(&mut child)?;
        let edge = child.i32()?;
        let vertices = [child.i32()?, child.i32()?];
        let reversed_3d = child.i32()?;
        let trim_type = child.i32()?;
        let iso = child.i32()?;
        let loop_index = child.i32()?;
        let tolerances = [child.f64()?, child.f64()?];
        let (domain, proxy_reversed, reserved) = if current {
            let domain = interval(&mut child)?;
            let proxy_reversed = child.u8()?;
            let reserved = child.take(31)?.to_vec();
            (domain, proxy_reversed, reserved)
        } else {
            child.skip(48)?;
            (proxy_domain, 0, Vec::new())
        };
        let legacy_tolerances = [child.f64()?, child.f64()?];
        result.push(RawBrepTrim {
            index,
            curve,
            proxy_domain,
            edge,
            vertices,
            reversed_3d,
            trim_type,
            iso,
            loop_index,
            tolerances,
            domain,
            proxy_reversed,
            reserved,
            legacy_tolerances,
            source_range: start..child.position(),
        });
    }
    let range = chunk.range();
    finish_anonymous(bytes, reader, &chunk, child)?;
    Ok((result, range))
}

fn read_loops(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<(Vec<RawBrepLoop>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let count = raw_array_start(&mut child, "loop", 20)?;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let start = child.position();
        let index = child.i32()?;
        let trims = indexes(&mut child, "loop trim")?;
        let loop_type = child.i32()?;
        let face = child.i32()?;
        result.push(RawBrepLoop {
            index,
            trims,
            loop_type,
            face,
            source_range: start..child.position(),
        });
    }
    let range = chunk.range();
    finish_anonymous(bytes, reader, &chunk, child)?;
    Ok((result, range))
}

fn read_faces(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<(Vec<RawBrepFace>, Range<usize>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let version = child.u8()?;
    if version >> 4 != 1 || version & 0x0f > 2 {
        return Err(unsupported(
            child.position() - 1,
            "unsupported Brep face-array version",
        ));
    }
    let count = count(&mut child, MAX_BREP_ITEMS)?;
    if count
        .checked_mul(20)
        .is_none_or(|bytes| bytes > child.remaining())
    {
        return Err(error(
            child.position(),
            "face count exhausts payload before allocation",
        ));
    }
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let record_start = child.position();
        let index = child.i32()?;
        let loops = indexes(&mut child, "face loop")?;
        let surface = child.i32()?;
        let reversed_surface = child.i32()?;
        let material_channel = child.i32()?;
        result.push(RawBrepFace {
            index,
            loops,
            surface,
            reversed_surface,
            material_channel,
            uuid: None,
            color: None,
            source_range: record_start..child.position(),
        });
    }
    if version & 0x0f >= 1 {
        for face in &mut result {
            face.uuid = Some(uuid(&mut child)?);
        }
    }
    if version & 0x0f >= 2 {
        let present = child.u8()?;
        if present > 1 {
            return Err(error(child.position() - 1, "invalid face-color presence"));
        }
        if present != 0 {
            for face in &mut result {
                face.color = Some(child.take(4)?.try_into().expect("color width checked"));
            }
        }
    }
    let range = chunk.range();
    finish_anonymous(bytes, reader, &chunk, child)?;
    Ok((result, range))
}

fn read_mesh_sides(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    face_count: usize,
    warnings: &mut Vec<String>,
) -> Result<Vec<RawBrepMeshSlot>, GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let parsed = (|| {
        if child.u8()? != 0 {
            return Err(unsupported(
                child.position() - 1,
                "unsupported Brep mesh-side version",
            ));
        }
        let mut result = Vec::with_capacity(face_count);
        for _ in 0..face_count {
            let present = child.bool()?;
            let mesh = if present {
                let start = child.position();
                let object = chunk_at(bytes, start, child.end(), archive, false)?;
                let class = parse_class_wrapper(
                    bytes,
                    chunk_start_range(&object),
                    archive,
                    &mut Vec::new(),
                );
                child.skip(object.next_offset - start)?;
                match class {
                    Ok(class) if supported_mesh(class.class_uuid) => Some(RawBrepChild {
                        class_uuid: class.class_uuid,
                        class_data_range: class.class_data_range,
                        source_range: start..object.next_offset,
                        base_type: RawBrepBaseType::Other,
                    }),
                    Ok(_) => {
                        warnings.push("Brep mesh cache slot has wrong class".to_string());
                        None
                    }
                    Err(error) => {
                        warnings.push(format!("Brep mesh cache slot degraded: {error}"));
                        None
                    }
                }
            } else {
                None
            };
            result.push(RawBrepMeshSlot { mesh, present });
        }
        finish_anonymous(bytes, reader, &chunk, child)?;
        Ok(result)
    })();
    match parsed {
        Ok(result) => Ok(result),
        Err(error) => {
            reader.skip(chunk.next_offset - reader.position())?;
            warnings.push(format!("Brep mesh cache degraded: {error}"));
            Ok(vec![
                RawBrepMeshSlot {
                    mesh: None,
                    present: false,
                };
                face_count
            ])
        }
    }
}

fn read_regions(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
    face_count: usize,
    warnings: &mut Vec<String>,
) -> Result<(Vec<RawBrepFaceSide>, Vec<RawBrepRegion>), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut outer = body_reader(bytes, &chunk)?;
    let parsed = (|| {
        if outer.i32()? != 1 || outer.i32()? != 0 {
            return Err(unsupported(
                outer.position() - 8,
                "unsupported Brep region wrapper",
            ));
        }
        if outer.bool()? == false {
            if outer.remaining() != 0 {
                return Err(error(outer.position(), "region wrapper has trailing bytes"));
            }
            return Ok((Vec::new(), Vec::new()));
        }
        let nested_chunk = anonymous_chunk(bytes, &mut outer, archive)?;
        let mut topology = body_reader(bytes, &nested_chunk)?;
        if topology.i32()? != 1 || topology.i32()? != 0 {
            return Err(unsupported(
                topology.position() - 8,
                "unsupported Brep region-topology version",
            ));
        }
        let sides = read_region_sides(bytes, &mut topology, archive)?;
        let regions = read_region_records(bytes, &mut topology, archive)?;
        finish_anonymous(bytes, &mut outer, &nested_chunk, topology)?;
        if sides.len() != face_count.saturating_mul(2) {
            return Err(error(outer.position(), "region face-side count mismatch"));
        }
        if outer.remaining() != 0 {
            return Err(error(outer.position(), "region wrapper has trailing bytes"));
        }
        Ok((sides, regions))
    })();
    reader.skip(chunk.next_offset - reader.position())?;
    match parsed {
        Ok(value) => {
            if matches!(
                verify_checksum(bytes, &chunk)?,
                ChecksumStatus::Mismatch { .. }
            ) {
                warnings.push("Brep region wrapper checksum mismatch".to_string());
            }
            Ok(value)
        }
        Err(error) => {
            warnings.push(format!(
                "invalid optional Brep region topology discarded: {error}"
            ));
            Ok((Vec::new(), Vec::new()))
        }
    }
}

fn read_region_sides<'a>(
    bytes: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
) -> Result<Vec<RawBrepFaceSide>, GeometryError> {
    let (chunk, mut child, count) = region_array(bytes, reader, archive)?;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let (body, source) = region_element(bytes, &mut child, archive)?;
        let mut child = BoundedReader::new(bytes, body.start, body.end)?;
        result.push(RawBrepFaceSide {
            index: child.i32()?,
            region: child.i32()?,
            face: child.i32()?,
            direction: child.i32()?,
            source_range: source,
        });
        if child.remaining() != 0 {
            return Err(error(
                child.position(),
                "region face-side has trailing bytes",
            ));
        }
    }
    finish_anonymous(bytes, reader, &chunk, child)?;
    Ok(result)
}

fn read_region_records<'a>(
    bytes: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
) -> Result<Vec<RawBrepRegion>, GeometryError> {
    let (chunk, mut child, count) = region_array(bytes, reader, archive)?;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let (body, source) = region_element(bytes, &mut child, archive)?;
        let mut child = BoundedReader::new(bytes, body.start, body.end)?;
        let index = child.i32()?;
        let region_type = child.i32()?;
        let sides = indexes(&mut child, "region side")?;
        let bounds = bbox(&mut child)?;
        if child.remaining() != 0 {
            return Err(error(child.position(), "region has trailing bytes"));
        }
        result.push(RawBrepRegion {
            index,
            region_type,
            sides,
            bounds,
            source_range: source,
        });
    }
    finish_anonymous(bytes, reader, &chunk, child)?;
    Ok(result)
}

fn region_array<'a>(
    bytes: &'a [u8],
    reader: &mut BoundedReader<'a>,
    archive: ArchiveVersion,
) -> Result<(Chunk, BoundedReader<'a>, usize), GeometryError> {
    let chunk = anonymous_chunk(bytes, reader, archive)?;
    let mut child = body_reader(bytes, &chunk)?;
    let count = anonymous_array_start(&mut child)?;
    Ok((chunk, child, count))
}

fn region_element(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<(Range<usize>, Range<usize>), GeometryError> {
    let start = reader.position();
    if archive.value() < 60 {
        let chunk = crate::chunks::chunk_at(bytes, start, reader.end(), archive, false)?;
        reader.skip(chunk.next_offset - start)?;
        let mut child = BoundedReader::new(bytes, chunk.body.start, chunk.body.end)?;
        let major = child.i32()?;
        let minor = child.i32()?;
        if major != 1 || minor != 0 {
            return Err(unsupported(start, "unsupported raw region element version"));
        }
        Ok((child.position()..chunk.body.end, start..chunk.next_offset))
    } else {
        let chunk = crate::chunks::chunk_at(bytes, start, reader.end(), archive, false)?;
        let class =
            parse_class_wrapper(bytes, chunk_start_range(&chunk), archive, &mut Vec::new())?;
        reader.skip(chunk.next_offset - start)?;
        Ok((class.class_data_range, start..chunk.next_offset))
    }
}

fn validate(mut raw: RawBrep) -> Result<ValidatedRawBrep, GeometryError> {
    positional(&raw.vertices, |v| v.index)?;
    positional(&raw.edges, |v| v.index)?;
    positional(&raw.trims, |v| v.index)?;
    positional(&raw.loops, |v| v.index)?;
    positional(&raw.faces, |v| v.index)?;
    for vertex in &raw.vertices {
        refs(&vertex.edges, raw.edges.len(), "vertex edge")?;
        finite_tolerance(vertex.tolerance, "vertex tolerance")?;
    }
    for (index, edge) in raw.edges.iter().enumerate() {
        if !typed_slot(&raw.c3, edge.curve, RawBrepBaseType::Curve) {
            return Err(error(
                edge.source_range.start,
                "edge C3 reference is invalid",
            ));
        }
        refs(&edge.vertices, raw.vertices.len(), "edge vertex")?;
        refs(&edge.trims, raw.trims.len(), "edge trim")?;
        unique(&edge.trims, "edge trim")?;
        finite_interval(edge.proxy_domain, "edge proxy domain")?;
        finite_interval(edge.domain, "edge domain")?;
        finite_tolerance(edge.tolerance, "edge tolerance")?;
        if edge.proxy_reversed != 0 && edge.proxy_reversed != 1 {
            return Err(error(
                edge.source_range.start,
                "invalid edge proxy reversal",
            ));
        }
        for trim in &edge.trims {
            if !raw.trims[*trim as usize].edge.eq(&(index as i32)) {
                return Err(error(
                    edge.source_range.start,
                    "edge/trim reciprocity mismatch",
                ));
            }
        }
    }
    for trim in &raw.trims {
        if trim.trim_type == 6 {
            if trim.curve != -1 {
                return Err(error(
                    trim.source_range.start,
                    "point-on-surface trim must not require C2",
                ));
            }
        } else if !typed_slot(&raw.c2, trim.curve, RawBrepBaseType::Curve) {
            return Err(error(
                trim.source_range.start,
                "trim C2 reference is invalid",
            ));
        }
        refs(&trim.vertices, raw.vertices.len(), "trim vertex")?;
        refs(&[trim.loop_index], raw.loops.len(), "trim loop")?;
        if !raw.loops[trim.loop_index as usize]
            .trims
            .contains(&trim.index)
        {
            return Err(error(
                trim.source_range.start,
                "trim/loop reciprocity mismatch",
            ));
        }
        finite_interval(trim.proxy_domain, "trim proxy domain")?;
        finite_interval(trim.domain, "trim domain")?;
        for tolerance in trim.tolerances.into_iter().chain(trim.legacy_tolerances) {
            finite_tolerance(tolerance, "trim tolerance")?;
        }
        if trim.proxy_reversed > 1 || trim.reversed_3d != 0 && trim.reversed_3d != 1 {
            return Err(error(trim.source_range.start, "invalid trim reversal"));
        }
        if !(0..=7).contains(&trim.trim_type) || !(0..=6).contains(&trim.iso) {
            return Err(error(trim.source_range.start, "invalid trim enum value"));
        }
        if matches!(trim.trim_type, 4 | 6) {
            if trim.edge != -1 || trim.vertices[0] != trim.vertices[1] {
                return Err(error(
                    trim.source_range.start,
                    "singular trim endpoints are invalid",
                ));
            }
        } else {
            refs(&[trim.edge], raw.edges.len(), "trim edge")?;
        }
    }
    validate_edge_incidences(&raw)?;
    for (index, vertex) in raw.vertices.iter().enumerate() {
        for edge in &vertex.edges {
            if !raw.edges[*edge as usize].vertices.contains(&(index as i32)) {
                return Err(error(
                    vertex.source_range.start,
                    "vertex/edge reciprocity mismatch",
                ));
            }
        }
    }
    for (index, loop_record) in raw.loops.iter().enumerate() {
        refs(&loop_record.trims, raw.trims.len(), "loop trim")?;
        unique(&loop_record.trims, "loop trim")?;
        refs(&[loop_record.face], raw.faces.len(), "loop face")?;
        if !(0..=5).contains(&loop_record.loop_type) {
            return Err(error(
                loop_record.source_range.start,
                "invalid loop enum value",
            ));
        }
        if !raw.faces[loop_record.face as usize]
            .loops
            .contains(&(index as i32))
        {
            return Err(error(
                loop_record.source_range.start,
                "loop/face reciprocity mismatch",
            ));
        }
        if loop_record.loop_type == 1
            && raw.faces[loop_record.face as usize]
                .loops
                .first()
                .is_none_or(|first| *first != index as i32)
        {
            return Err(error(
                loop_record.source_range.start,
                "outer loop is not first",
            ));
        }
    }
    for face in &mut raw.faces {
        if face.material_channel < 0 {
            face.material_channel = 0;
        }
    }
    for (index, face) in raw.faces.iter().enumerate() {
        if !typed_slot(&raw.surfaces, face.surface, RawBrepBaseType::Surface) {
            return Err(error(
                face.source_range.start,
                "face surface reference is invalid",
            ));
        }
        refs(&face.loops, raw.loops.len(), "face loop")?;
        if face.loops.is_empty() {
            return Err(error(face.source_range.start, "face has no loops"));
        }
        if raw.loops[face.loops[0] as usize].loop_type != 1 {
            return Err(error(
                face.source_range.start,
                "face first loop is not outer",
            ));
        }
        for loop_index in face.loops.iter().skip(1) {
            let loop_type = raw.loops[*loop_index as usize].loop_type;
            if loop_type == 0 || loop_type == 1 {
                return Err(error(
                    face.source_range.start,
                    "face boundary loop convention is invalid",
                ));
            }
        }
        for loop_index in &face.loops {
            if raw.loops[*loop_index as usize].face != index as i32 {
                return Err(error(
                    face.source_range.start,
                    "face/loop reciprocity mismatch",
                ));
            }
        }
    }
    validate_rings(&raw)?;
    let mut warnings = Vec::new();
    if raw.minor >= 3
        && (!raw.face_sides.is_empty() || !raw.regions.is_empty())
        && validate_regions(&raw).is_err()
    {
        raw.face_sides.clear();
        raw.regions.clear();
        warnings.push("invalid optional Brep region topology discarded".to_string());
    }
    Ok(ValidatedRawBrep { raw, warnings })
}

fn validate_rings(raw: &RawBrep) -> Result<(), GeometryError> {
    for loop_record in &raw.loops {
        if loop_record.trims.is_empty() {
            return Err(error(loop_record.source_range.start, "loop ring is empty"));
        }
        for pair in loop_record.trims.windows(2) {
            let left = &raw.trims[pair[0] as usize];
            let right = &raw.trims[pair[1] as usize];
            let left_end = left.vertices[1 - usize::from(left.reversed_3d != 0)];
            let right_start = right.vertices[usize::from(right.reversed_3d != 0)];
            if left_end != right_start {
                return Err(error(
                    loop_record.source_range.start,
                    "loop ring is discontinuous",
                ));
            }
        }
        let first = &raw.trims[loop_record.trims[0] as usize];
        let last = &raw.trims[*loop_record.trims.last().expect("nonempty") as usize];
        let first_start = first.vertices[usize::from(first.reversed_3d != 0)];
        let last_end = last.vertices[1 - usize::from(last.reversed_3d != 0)];
        if first_start != last_end {
            return Err(error(
                loop_record.source_range.start,
                "loop ring is not closed",
            ));
        }
    }
    Ok(())
}

fn validate_regions(raw: &RawBrep) -> Result<(), GeometryError> {
    if raw.face_sides.len() != raw.faces.len().saturating_mul(2) {
        return Err(error(
            raw.source_range.start,
            "region side count is invalid",
        ));
    }
    let mut infinite = 0;
    for (index, side) in raw.face_sides.iter().enumerate() {
        if side.index != index as i32 || side.face < 0 || side.face as usize >= raw.faces.len() {
            return Err(error(
                side.source_range.start,
                "region face-side index is invalid",
            ));
        }
        let expected = if index % 2 == 0 { 1 } else { -1 };
        if side.direction != expected {
            return Err(error(
                side.source_range.start,
                "region side direction is invalid",
            ));
        }
        if side.face != (index / 2) as i32 {
            return Err(error(
                side.source_range.start,
                "region side face position is invalid",
            ));
        }
        if side.region < -1 || side.region as usize >= raw.regions.len() {
            return Err(error(
                side.source_range.start,
                "region membership is invalid",
            ));
        }
    }
    for (index, region) in raw.regions.iter().enumerate() {
        if region.index != index as i32 || !matches!(region.region_type, 0 | 1) {
            return Err(error(region.source_range.start, "region record is invalid"));
        }
        if region.region_type == 0 {
            infinite += 1;
        }
        let mut seen = Vec::new();
        for side in &region.sides {
            refs(&[*side], raw.face_sides.len(), "region side")?;
            if seen.contains(side) || raw.face_sides[*side as usize].region != index as i32 {
                return Err(error(
                    region.source_range.start,
                    "region membership is not reciprocal",
                ));
            }
            seen.push(*side);
        }
    }
    if infinite != 1 {
        return Err(error(
            raw.source_range.start,
            "region topology needs one infinite region",
        ));
    }
    Ok(())
}

fn raw_array_start(
    reader: &mut BoundedReader<'_>,
    label: &str,
    minimum_record_bytes: usize,
) -> Result<usize, GeometryError> {
    let version = reader.u8()?;
    if version != 0x10 {
        return Err(unsupported(
            reader.position() - 1,
            &format!("unsupported {label} array version"),
        ));
    }
    let count = count(reader, MAX_BREP_ITEMS)?;
    if count
        .checked_mul(minimum_record_bytes)
        .is_none_or(|bytes| bytes > reader.remaining())
    {
        return Err(error(
            reader.position(),
            &format!("{label} count exhausts payload before allocation"),
        ));
    }
    Ok(count)
}

fn anonymous_array_start(reader: &mut BoundedReader<'_>) -> Result<usize, GeometryError> {
    let major = reader.i32()?;
    let minor = reader.i32()?;
    if major != 1 || minor != 0 {
        return Err(unsupported(
            reader.position() - 8,
            "unsupported region array version",
        ));
    }
    count(reader, MAX_BREP_ITEMS)
}

fn indexes(reader: &mut BoundedReader<'_>, label: &str) -> Result<Vec<i32>, GeometryError> {
    let count = count(reader, MAX_BREP_ITEMS)?;
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        result.push(reader.i32()?);
    }
    let _ = label;
    Ok(result)
}

fn count(reader: &mut BoundedReader<'_>, cap: usize) -> Result<usize, GeometryError> {
    let value = reader.i32()?;
    if value < 0 {
        return Err(error(reader.position() - 4, "Brep count exceeds cap"));
    }
    let count = usize::try_from(value).map_err(|_| error(reader.position(), "count overflow"))?;
    if count > cap {
        return Err(error(reader.position() - 4, "Brep count exceeds cap"));
    }
    let minimum = count
        .checked_mul(4)
        .ok_or_else(|| error(reader.position(), "count overflow"))?;
    if minimum > reader.remaining() {
        return Err(error(reader.position(), "Brep count exhausts payload"));
    }
    Ok(count)
}

fn positional<T>(values: &[T], index: impl Fn(&T) -> i32) -> Result<(), GeometryError> {
    for (position, value) in values.iter().enumerate() {
        if index(value) != position as i32 {
            return Err(error(position, "Brep positional index mismatch"));
        }
    }
    Ok(())
}

fn refs(values: &[i32], len: usize, label: &str) -> Result<(), GeometryError> {
    if values
        .iter()
        .any(|value| *value < 0 || (*value as usize) >= len)
    {
        return Err(error(0, &format!("{label} reference is out of range")));
    }
    Ok(())
}

fn typed_slot(array: &RawBrepChildren, index: i32, expected: RawBrepBaseType) -> bool {
    index >= 0
        && array
            .slots
            .get(index as usize)
            .and_then(Option::as_ref)
            .is_some_and(|child| child.base_type == expected)
}

fn validate_edge_incidences(raw: &RawBrep) -> Result<(), GeometryError> {
    let mut actual = vec![Vec::new(); raw.vertices.len()];
    for (vertex, record) in raw.vertices.iter().enumerate() {
        for edge in &record.edges {
            actual[vertex].push(*edge);
        }
    }
    for (edge_index, edge) in raw.edges.iter().enumerate() {
        for trim_index in &edge.trims {
            let trim = &raw.trims[*trim_index as usize];
            if trim.edge >= 0
                && !((trim.vertices[0] == edge.vertices[0] && trim.vertices[1] == edge.vertices[1])
                    || (trim.vertices[0] == edge.vertices[1]
                        && trim.vertices[1] == edge.vertices[0]))
            {
                return Err(error(
                    edge.source_range.start,
                    "edge/trim endpoint incidence mismatch",
                ));
            }
        }
        for (endpoint, vertex) in edge.vertices.iter().enumerate() {
            let expected = if edge.vertices[0] == edge.vertices[1] {
                2
            } else {
                1
            };
            let count = actual[*vertex as usize]
                .iter()
                .filter(|value| **value == edge_index as i32)
                .count();
            if count != expected {
                return Err(error(
                    edge.source_range.start,
                    if edge.vertices[0] == edge.vertices[1] && endpoint == 1 {
                        "closed edge incidence is duplicated incorrectly"
                    } else {
                        "edge/vertex incidence mismatch"
                    },
                ));
            }
        }
    }
    Ok(())
}

fn unique(values: &[i32], label: &str) -> Result<(), GeometryError> {
    for (index, value) in values.iter().enumerate() {
        if values[..index].contains(value) {
            return Err(error(0, &format!("{label} reference is duplicated")));
        }
    }
    Ok(())
}

fn finite_interval(value: Interval, label: &str) -> Result<(), GeometryError> {
    let [low, high] = value.0;
    let unset = low == ON_UNSET_VALUE && high == ON_UNSET_VALUE;
    let empty = low == ON_UNSET_VALUE && high == ON_UNSET_NEGATIVE_VALUE;
    if !(unset || empty || low.is_finite() && high.is_finite() && low < high) {
        return Err(error(0, &format!("{label} is invalid")));
    }
    Ok(())
}

fn finite_tolerance(value: f64, label: &str) -> Result<(), GeometryError> {
    if !(value == ON_UNSET_VALUE || value.is_finite() && value >= 0.0) {
        return Err(error(0, &format!("{label} is invalid")));
    }
    Ok(())
}

fn point(reader: &mut BoundedReader<'_>) -> Result<Point3, GeometryError> {
    let point = Point3([reader.f64()?, reader.f64()?, reader.f64()?]);
    if point.0.iter().any(|value| !value.is_finite()) {
        return Err(error(reader.position() - 24, "Brep point is not finite"));
    }
    Ok(point)
}

fn uuid(reader: &mut BoundedReader<'_>) -> Result<Uuid, GeometryError> {
    Ok(Uuid::from_wire(
        reader.take(16)?.try_into().expect("UUID width checked"),
    ))
}

fn supported_mesh(uuid: Uuid) -> bool {
    uuid.to_string() == crate::mesh::ON_MESH
}

fn chunk_start_range(chunk: &crate::chunks::Chunk) -> Range<usize> {
    chunk.range()
}

fn anonymous_chunk(
    bytes: &[u8],
    reader: &mut BoundedReader<'_>,
    archive: ArchiveVersion,
) -> Result<Chunk, GeometryError> {
    let chunk = chunk_at(bytes, reader.position(), reader.end(), archive, false)?;
    if chunk.typecode != ANONYMOUS || chunk.short {
        return Err(error(
            chunk.header_start,
            "expected bounded anonymous Brep chunk",
        ));
    }
    Ok(chunk)
}

fn body_reader<'a>(bytes: &'a [u8], chunk: &Chunk) -> Result<BoundedReader<'a>, GeometryError> {
    Ok(BoundedReader::new(bytes, chunk.body.start, chunk.body.end)?)
}

fn finish_anonymous(
    bytes: &[u8],
    parent: &mut BoundedReader<'_>,
    chunk: &Chunk,
    child: BoundedReader<'_>,
) -> Result<(), GeometryError> {
    if child.remaining() != 0 {
        return Err(error(
            child.position(),
            "anonymous Brep chunk has trailing bytes",
        ));
    }
    if matches!(
        verify_checksum(bytes, chunk)?,
        ChecksumStatus::Mismatch { .. }
    ) {
        // A checksum mismatch is recoverable once the declared boundary is
        // valid. The containing Brep remains structurally consumable.
    }
    parent.skip(chunk.next_offset - parent.position())?;
    Ok(())
}

fn classify_base_type(uuid: Uuid) -> RawBrepBaseType {
    let name = uuid.to_string();
    if matches!(
        name.as_str(),
        "4ed7d4d9-e947-11d3-bfe5-0010830122f0"
            | "4ed7d4d8-e947-11d3-bfe5-0010830122f0"
            | "4ed7d4dd-e947-11d3-bfe5-0010830122f0"
            | "4ed7d4db-e947-11d3-bfe5-0010830122f0"
            | "cf33be2a-09b4-11d4-bffb-0010830122f0"
            | "4ed7d4e6-e947-11d3-bfe5-0010830122f0"
            | "4ed7d4e0-e947-11d3-bfe5-0010830122f0"
    ) {
        RawBrepBaseType::Curve
    } else if matches!(
        name.as_str(),
        "4ed7d4de-e947-11d3-bfe5-0010830122f0"
            | "4ed7d4df-e947-11d3-bfe5-0010830122f0"
            | "a16220d3-163b-11d4-8000-0010830122f0"
            | "c4cd5359-446d-4690-9ff5-29059732472b"
    ) {
        RawBrepBaseType::Surface
    } else {
        RawBrepBaseType::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anonymous(body: &[u8]) -> Vec<u8> {
        let mut bytes = 0x4000_8000_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&i64::try_from(body.len() + 4).expect("length").to_le_bytes());
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(&crc32fast::hash(body).to_le_bytes());
        bytes
    }

    fn packed_array(count: i32, records: &[u8]) -> Vec<u8> {
        let mut body = vec![0x10];
        body.extend_from_slice(&count.to_le_bytes());
        body.extend_from_slice(records);
        anonymous(&body)
    }

    fn class_wrapper(data: &[u8]) -> Vec<u8> {
        let mut uuid = 0x0002_fffb_u32.to_le_bytes().to_vec();
        uuid.extend_from_slice(&20_i64.to_le_bytes());
        uuid.extend([0; 16]);
        uuid.extend_from_slice(&crc32fast::hash(&[0; 16]).to_le_bytes());
        let mut class_data = 0x0002_fffc_u32.to_le_bytes().to_vec();
        class_data.extend_from_slice(&i64::try_from(data.len() + 4).expect("length").to_le_bytes());
        class_data.extend_from_slice(data);
        class_data.extend_from_slice(&crc32fast::hash(data).to_le_bytes());
        let mut end = 0x8202_7fff_u32.to_le_bytes().to_vec();
        end.extend_from_slice(&0_i64.to_le_bytes());
        let mut body = uuid;
        body.extend(class_data);
        body.extend(end);
        let mut wrapper = 0x0002_7ffa_u32.to_le_bytes().to_vec();
        wrapper.extend_from_slice(&i64::try_from(body.len()).expect("length").to_le_bytes());
        wrapper.extend(body);
        wrapper
    }

    fn interval_bytes() -> Vec<u8> {
        [0.0_f64, 1.0]
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect()
    }

    fn trim_record(current: bool) -> Vec<u8> {
        let mut record = Vec::new();
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend(interval_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&1_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&1_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend_from_slice(&0_i32.to_le_bytes());
        record.extend([0.0_f64, 0.0].into_iter().flat_map(f64::to_le_bytes));
        if current {
            record.extend(interval_bytes());
            record.push(0);
            record.extend([0; 31]);
        } else {
            record.extend([0_u8; 48]);
        }
        record.extend([0.0_f64, 0.0].into_iter().flat_map(f64::to_le_bytes));
        record
    }

    #[test]
    fn brep_major_is_structured_as_unsupported() {
        let error =
            parse(&[0x20], 0..1, ArchiveVersion::V5, None).expect_err("major two must be rejected");
        assert!(matches!(
            error,
            GeometryError::UnsupportedVersion { offset: 0, .. }
        ));
    }

    #[test]
    fn negative_array_count_is_rejected_before_allocation() {
        let mut bytes = vec![0x30];
        bytes.extend_from_slice(&1_i32.to_le_bytes());
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        bytes.extend_from_slice(&(-1_i32).to_le_bytes());
        let error = parse(&bytes, 0..bytes.len(), ArchiveVersion::V5, None)
            .expect_err("negative C2 count must fail");
        assert!(matches!(error, GeometryError::Malformed(_)));
    }

    #[test]
    fn raw_arrays_consume_complete_anonymous_wrappers() {
        let bytes = packed_array(0, &[]);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let (_, range) = read_vertices(&bytes, &mut reader, ArchiveVersion::V5).expect("vertex");
        assert_eq!(range, 0..bytes.len());
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn face_reader_accepts_all_packed_minors() {
        for version in [0x10_u8, 0x11, 0x12] {
            let mut body = vec![version, 0, 0, 0, 0];
            if version == 0x12 {
                body.push(0);
            }
            let bytes = anonymous(&body);
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
            let (faces, _) = read_faces(&bytes, &mut reader, ArchiveVersion::V5).expect("faces");
            assert!(faces.is_empty());
        }
    }

    #[test]
    fn trim_gate_preserves_legacy_tail_and_wrapper_range() {
        for writer in [200_000_000_i64, 200_206_180] {
            let record = trim_record(writer >= 200_206_180);
            assert_eq!(record.len(), 132);
            let bytes = packed_array(1, &record);
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
            let (trims, range) =
                read_trims(&bytes, &mut reader, ArchiveVersion::V5, Some(writer)).expect("trims");
            assert_eq!(range, 0..bytes.len());
            assert_eq!(trims[0].legacy_tolerances, [0.0, 0.0]);
        }
    }

    #[test]
    fn mesh_side_wrapper_rejects_non_boolean_presence_without_losing_parent() {
        let bytes = anonymous(&[0, 2]);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let mut warnings = Vec::new();
        let slots = read_mesh_sides(&bytes, &mut reader, ArchiveVersion::V5, 1, &mut warnings)
            .expect("degraded cache");
        assert!(slots[0].mesh.is_none());
        assert!(!warnings.is_empty());
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn polymorphic_array_preserves_null_and_classifies_wrong_base() {
        let mut body = 1_i32.to_le_bytes().to_vec();
        body.extend_from_slice(&0_i32.to_le_bytes());
        body.extend_from_slice(&2_i32.to_le_bytes());
        body.extend_from_slice(&0_i32.to_le_bytes());
        body.extend_from_slice(&1_i32.to_le_bytes());
        body.extend(class_wrapper(&[]));
        let bytes = anonymous(&body);
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).expect("reader");
        let array = read_children(
            &bytes,
            &mut reader,
            ArchiveVersion::V5,
            RawBrepBaseType::Curve,
            0,
        )
        .expect("children");
        assert!(array.slots[0].is_none());
        assert_eq!(
            array.slots[1].as_ref().expect("wrong class").base_type,
            RawBrepBaseType::Other
        );
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn region_outer_wrapper_preserves_v5_raw_element_boundaries() {
        let mut region_record = Vec::new();
        region_record.extend_from_slice(&0_i32.to_le_bytes());
        region_record.extend_from_slice(&0_i32.to_le_bytes());
        region_record.extend_from_slice(&0_i32.to_le_bytes());
        region_record.extend([0.0_f64; 6].into_iter().flat_map(f64::to_le_bytes));
        let raw_element = anonymous(&{
            let mut body = 1_i32.to_le_bytes().to_vec();
            body.extend_from_slice(&0_i32.to_le_bytes());
            body.extend(region_record);
            body
        });
        let region_array = anonymous(&{
            let mut body = 1_i32.to_le_bytes().to_vec();
            body.extend_from_slice(&0_i32.to_le_bytes());
            body.extend_from_slice(&1_i32.to_le_bytes());
            body.extend(raw_element);
            body
        });
        let side_array = anonymous(&[0, 0, 0, 0, 0, 0, 0, 0]);
        let nested = anonymous(&{
            let mut body = 1_i32.to_le_bytes().to_vec();
            body.extend_from_slice(&0_i32.to_le_bytes());
            body.extend(side_array);
            body.extend(region_array);
            body
        });
        let outer = anonymous(&{
            let mut body = 1_i32.to_le_bytes().to_vec();
            body.extend_from_slice(&0_i32.to_le_bytes());
            body.push(1);
            body.extend(nested);
            body
        });
        let mut reader = BoundedReader::new(&outer, 0, outer.len()).expect("reader");
        let mut warnings = Vec::new();
        let (_, regions) = read_regions(&outer, &mut reader, ArchiveVersion::V5, 0, &mut warnings)
            .expect("regions");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(regions.len(), 1);
        assert_eq!(reader.remaining(), 0);
    }
}
