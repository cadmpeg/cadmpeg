// SPDX-License-Identifier: Apache-2.0
//! JT display-model record extractors and their record types.

pub(crate) mod admission;
#[cfg(test)]
mod document_admission_tests;
#[cfg(test)]
mod index_admission_tests;
mod packet_role;
#[cfg(test)]
mod scene_admission_tests;
#[cfg(test)]
mod segment_admission_tests;
mod version;

use std::collections::{BTreeMap, BTreeSet};

use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};

use crate::container::Container;
use cadmpeg_core::CodecError;

use cadmpeg_ir::hash::digest::Sha256Digest;
use packet_role::{TopologyContext, TopologyPacketRole};
use version::JtVersionField;

use cadmpeg_container::compression::inflate_zlib_exact;
use cadmpeg_core::bytes::{assemble_f32_le, assemble_u32_le, assemble_u64_le};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteBinary32, UnitBinary32};
use cadmpeg_ir::tessellation::{Tessellation, TessellationChannel};
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::{topology::Color, SourceObjectAssociation};

use std::num::NonZeroU64;

use crate::jt::QuantizedRange;
use crate::jt_topology::Polygon;
use crate::layout::jt_document_header as jt_hdr;
use crate::layout::jt_toc_entry as jt_toc;
use crate::layout::jt_tristrip_shape_node_family_data as jt_family;
use crate::om::nonempty::NonEmpty;

fn replace_jt_text_once(
    ctx: &DecodeContext<'_>,
    source: &str,
    from: &str,
    to: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    let Some(index) = source.find(from) else {
        return ctx.join_retained(&[source], "", operation);
    };
    let after = index + from.len();
    ctx.join_retained(&[&source[..index], to, &source[after..]], "", operation)
}

fn retain_jt_tessellation_id(
    ctx: &DecodeContext<'_>,
    offset: u64,
    object_id: u32,
    path: Option<&str>,
) -> Result<cadmpeg_ir::tessellation::TessellationId, CodecError> {
    let operation = "nx JT tessellation identity";
    let digits = |mut value: u64| {
        let mut count = 1usize;
        while value >= 10 {
            value /= 10;
            count += 1;
        }
        count
    };
    let prefix = "nx:display-jt:tessellation#";
    let path_length = path
        .map_or(Some(0), |path| 6usize.checked_add(path.len()))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    let length = prefix
        .len()
        .checked_add(digits(offset))
        .and_then(|length| length.checked_add(1 + digits(u64::from(object_id))))
        .and_then(|length| length.checked_add(path_length))
        .ok_or_else(|| ctx.refuse_codec_limit(operation, 0, u64::MAX))?;
    let mut id = ctx.retained_string(length, operation)?;
    id.push_str(prefix);
    std::fmt::Write::write_fmt(&mut id, format_args!("{offset}-{object_id}")).map_err(|_| {
        ctx.refuse_codec_limit(operation, 0, cadmpeg_core::decode::u64_from_index(length))
    })?;
    if let Some(path) = path {
        id.push_str("-path-");
        id.push_str(path);
    }
    cadmpeg_ir::tessellation::TessellationId::mint(id)
        .map_err(|error| CodecError::malformed(format_args!("display-jt tessellation: {error}")))
}

fn display_jt_text_size(
    ctx: &DecodeContext<'_>,
    parts: &[&str],
    digits: u64,
) -> Result<u64, CodecError> {
    let invalid = || ctx.refuse_codec_limit("size DisplayJT text", 0, digits);
    parts.iter().try_fold(digits, |sum, part| {
        let len = u64::try_from(part.len()).map_err(|_| invalid())?;
        sum.checked_add(len).ok_or_else(invalid)
    })
}

fn inflate_display_jt(
    ctx: &DecodeContext<'_>,
    member: View<'_>,
) -> Result<Option<Vec<u8>>, CodecError> {
    let view = match inflate_zlib_exact(ctx, member) {
        Ok(view) => view,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    ctx.copy_retained(view.window(), "retain inflated DisplayJT payload")
        .map(Some)
}

/// Outer index of the embedded JT display-model stream.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DisplayJtIndexWire")]
pub(super) struct DisplayJtIndex {
    /// Globally unique index identity.
    pub(super) id: String,
    /// Serialized index version.
    version: u32,
    /// Indexed document rows in serialized order.
    rows: NonEmpty<DisplayJtIndexRow>,
    /// Absolute source offset of the `DisplayJT` payload.
    pub(super) source_offset: u64,
}

#[cfg(test)]
std::thread_local! {
    static JT_INDEX_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DisplayJtIndex {
    fn clone(&self) -> Self {
        JT_INDEX_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            version: self.version,
            rows: self.rows.clone(),
            source_offset: self.source_offset,
        }
    }
}

impl DisplayJtIndex {
    fn new(
        id: String,
        version: u32,
        rows: Vec<DisplayJtIndexRow>,
        source_offset: u64,
    ) -> Result<Self, &'static str> {
        u32::try_from(rows.len()).map_err(|_| "rows: count exceeds u32")?;
        let rows = NonEmpty::from_vec(rows).ok_or("rows: at least one row is required")?;
        Ok(Self {
            id,
            version,
            rows,
            source_offset,
        })
    }

    /// Number of indexed JT documents.
    fn declared_count(&self) -> usize {
        self.rows.len()
    }

    /// Indexed document rows in serialized order.
    pub(super) fn rows(&self) -> impl Iterator<Item = &DisplayJtIndexRow> {
        self.rows.iter()
    }
}

struct DisplayJtIndexRows<'a>(&'a NonEmpty<DisplayJtIndexRow>);

impl Serialize for DisplayJtIndexRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut rows = serializer.serialize_seq(Some(self.0.len()))?;
        for row in self.0.iter() {
            rows.serialize_element(row)?;
        }
        rows.end()
    }
}

#[derive(Serialize)]
struct DisplayJtIndexRef<'a> {
    id: &'a str,
    version: u32,
    declared_count: usize,
    rows: DisplayJtIndexRows<'a>,
    source_offset: u64,
}

impl Serialize for DisplayJtIndex {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DisplayJtIndexRef {
            id: &self.id,
            version: self.version,
            declared_count: self.declared_count(),
            rows: DisplayJtIndexRows(&self.rows),
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct DisplayJtIndexWire {
    id: String,
    version: u32,
    declared_count: usize,
    rows: Vec<DisplayJtIndexRow>,
    source_offset: u64,
}

#[cfg(test)]
impl From<DisplayJtIndex> for DisplayJtIndexWire {
    fn from(value: DisplayJtIndex) -> Self {
        let declared_count = value.declared_count();
        Self {
            id: value.id,
            version: value.version,
            declared_count,
            rows: value.rows.into_iter().collect(),
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<DisplayJtIndexWire> for DisplayJtIndex {
    type Error = &'static str;

    fn try_from(wire: DisplayJtIndexWire) -> Result<Self, Self::Error> {
        if wire.declared_count != wire.rows.len() {
            return Err("declared_count: count must match rows");
        }
        Self::new(wire.id, wire.version, wire.rows, wire.source_offset)
    }
}

/// One physical-header offset and associated value in a `DisplayJT` index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) struct DisplayJtIndexRow {
    /// Globally unique row identity.
    pub(super) id: String,
    /// Zero-based row index.
    ordinal: u32,
    /// Payload-relative physical JT-header offset.
    header_offset: u32,
    /// Nonzero serialized row value whose semantic role is unassigned.
    value: NonZeroU64,
    /// Absolute source offset of the row.
    pub(super) source_offset: u64,
}

/// One bounded embedded JT document and its table of contents.
#[derive(Debug, PartialEq, Eq, Deserialize)]
#[cfg_attr(not(test), derive(Clone))]
#[serde(try_from = "DisplayJtDocumentWire")]
pub(in crate::native) struct DisplayJtDocument {
    /// Globally unique document identity.
    pub(super) id: String,
    /// Owning outer-index row.
    index_row: String,
    /// Exact admitted 80-byte version field.
    version: JtVersionField,
    /// Payload-relative table-of-contents offset.
    toc_offset: u32,
    /// Exact 16-byte logical scene-graph segment identifier.
    lsg_segment_id: [u8; 16],
    /// Ordered table-of-contents entries.
    pub(super) toc_entries: Vec<DisplayJtTocEntry>,
    /// Physical byte length ending at the next indexed header or stream boundary.
    physical_byte_len: u64,
    /// Absolute source offset of the JT version field.
    pub(super) source_offset: u64,
}

#[cfg(test)]
std::thread_local! {
    static JT_DOCUMENT_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl Clone for DisplayJtDocument {
    fn clone(&self) -> Self {
        JT_DOCUMENT_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: self.id.clone(),
            index_row: self.index_row.clone(),
            version: self.version.clone(),
            toc_offset: self.toc_offset,
            lsg_segment_id: self.lsg_segment_id,
            toc_entries: self.toc_entries.clone(),
            physical_byte_len: self.physical_byte_len,
            source_offset: self.source_offset,
        }
    }
}

#[derive(Serialize)]
struct DisplayJtDocumentRef<'a> {
    id: &'a str,
    index_row: &'a str,
    version_field: &'a str,
    format_major: u16,
    format_minor: u16,
    byte_order: u8,
    toc_offset: u32,
    lsg_segment_id: &'a [u8; 16],
    toc_entries: &'a [DisplayJtTocEntry],
    physical_byte_len: u64,
    source_offset: u64,
}

impl Serialize for DisplayJtDocument {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DisplayJtDocumentRef {
            id: &self.id,
            index_row: &self.index_row,
            version_field: self.version.as_str(),
            format_major: self.version.major(),
            format_minor: self.version.minor(),
            byte_order: 0,
            toc_offset: self.toc_offset,
            lsg_segment_id: &self.lsg_segment_id,
            toc_entries: &self.toc_entries,
            physical_byte_len: self.physical_byte_len,
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct DisplayJtDocumentWire {
    id: String,
    index_row: String,
    version_field: String,
    format_major: u16,
    format_minor: u16,
    byte_order: u8,
    toc_offset: u32,
    lsg_segment_id: [u8; 16],
    toc_entries: Vec<DisplayJtTocEntry>,
    physical_byte_len: u64,
    source_offset: u64,
}

#[cfg(test)]
impl From<DisplayJtDocument> for DisplayJtDocumentWire {
    fn from(value: DisplayJtDocument) -> Self {
        let format_major = value.version.major();
        let format_minor = value.version.minor();
        Self {
            id: value.id,
            index_row: value.index_row,
            version_field: value.version.into_string(),
            format_major,
            format_minor,
            byte_order: 0,
            toc_offset: value.toc_offset,
            lsg_segment_id: value.lsg_segment_id,
            toc_entries: value.toc_entries,
            physical_byte_len: value.physical_byte_len,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<DisplayJtDocumentWire> for DisplayJtDocument {
    type Error = &'static str;
    fn try_from(wire: DisplayJtDocumentWire) -> Result<Self, Self::Error> {
        if wire.byte_order != 0 {
            return Err("DisplayJtDocument.byte_order must be 0");
        }
        let version = JtVersionField::new(wire.version_field)?;
        if wire.format_major != version.major() || wire.format_minor != version.minor() {
            return Err("DisplayJtDocument.format_major/format_minor disagree with version_field");
        }
        Ok(Self {
            id: wire.id,
            index_row: wire.index_row,
            version,
            toc_offset: wire.toc_offset,
            lsg_segment_id: wire.lsg_segment_id,
            toc_entries: wire.toc_entries,
            physical_byte_len: wire.physical_byte_len,
            source_offset: wire.source_offset,
        })
    }
}

/// One fixed-width entry in an embedded JT document table of contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(in crate::native) struct DisplayJtTocEntry {
    /// Globally unique TOC-entry identity.
    pub(super) id: String,
    /// Zero-based serialized entry order.
    ordinal: u32,
    /// Exact 16-byte segment identifier.
    segment_id: [u8; 16],
    /// Document-relative segment offset.
    segment_offset: u32,
    /// Physical segment byte length.
    segment_byte_len: u32,
    /// Exact four-byte segment attribute field.
    attributes: [u8; 4],
    /// Absolute source offset of the TOC entry.
    pub(super) source_offset: u64,
}

/// One physically bounded segment in an embedded JT document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DisplayJtSegment {
    /// Globally unique segment identity.
    pub(super) id: String,
    /// Owning JT document.
    document: String,
    /// Owning table-of-contents entry.
    toc_entry: String,
    /// Exact 16-byte segment identifier.
    segment_id: [u8; 16],
    /// Segment type repeated by the table-of-contents attribute word.
    segment_type: u32,
    /// Physical segment byte length, including its 24-byte header.
    segment_byte_len: u32,
    /// SHA-256 of the bytes following the segment header.
    payload_sha256: Sha256Digest,
    /// Complete compressed-data envelope when the payload is compressed.
    compression: Option<DisplayJtCompression>,
    /// Absolute source offset of the segment header.
    pub(super) source_offset: u64,
}

/// Validated compressed-data envelope following a JT segment header.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DisplayJtCompressionWire")]
struct DisplayJtCompression {
    envelope: JtCompressionEnvelope,
    /// SHA-256 of the completely inflated payload.
    inflated_sha256: Sha256Digest,
}

#[derive(Serialize)]
struct DisplayJtCompressionRef<'a> {
    flag: u32,
    compressed_data_byte_len: u32,
    algorithm: u8,
    compressed_byte_len: u32,
    inflated_sha256: &'a Sha256Digest,
}

impl Serialize for DisplayJtCompression {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DisplayJtCompressionRef {
            flag: 2,
            compressed_data_byte_len: self.envelope.compressed_byte_len + 1,
            algorithm: 2,
            compressed_byte_len: self.envelope.compressed_byte_len,
            inflated_sha256: &self.inflated_sha256,
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct DisplayJtCompressionWire {
    flag: u32,
    compressed_data_byte_len: u32,
    algorithm: u8,
    compressed_byte_len: u32,
    inflated_sha256: Sha256Digest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct JtCompressionEnvelope {
    compressed_byte_len: u32,
}

impl JtCompressionEnvelope {
    fn try_new(
        flag: u32,
        compressed_data_byte_len: u32,
        algorithm: u8,
        compressed_byte_len: u32,
    ) -> Result<Self, &'static str> {
        if flag != 2 {
            return Err("DisplayJtCompression.flag must be 2");
        }
        if algorithm != 2 {
            return Err("DisplayJtCompression.algorithm must be 2");
        }
        if compressed_byte_len.checked_add(1) != Some(compressed_data_byte_len) {
            return Err(
                "DisplayJtCompression.compressed_data_byte_len disagrees with compressed_byte_len",
            );
        }
        Ok(Self {
            compressed_byte_len,
        })
    }
}

impl TryFrom<DisplayJtCompressionWire> for DisplayJtCompression {
    type Error = &'static str;
    fn try_from(wire: DisplayJtCompressionWire) -> Result<Self, Self::Error> {
        let envelope = JtCompressionEnvelope::try_new(
            wire.flag,
            wire.compressed_data_byte_len,
            wire.algorithm,
            wire.compressed_byte_len,
        )?;
        Ok(Self {
            envelope,
            inflated_sha256: wire.inflated_sha256,
        })
    }
}

#[cfg(test)]
std::thread_local! {
    static JT_COMPRESSION_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static JT_SHAPE_LOD_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static JT_COMPRESSED_ELEMENT_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl From<DisplayJtCompression> for DisplayJtCompressionWire {
    fn from(value: DisplayJtCompression) -> Self {
        JT_COMPRESSION_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            flag: 2,
            compressed_data_byte_len: value.envelope.compressed_byte_len + 1,
            algorithm: 2,
            compressed_byte_len: value.envelope.compressed_byte_len,
            inflated_sha256: value.inflated_sha256,
        }
    }
}

/// One length-bounded object element in a JT shape-LOD segment.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DisplayJtShapeLodElementWire")]
pub(super) struct DisplayJtShapeLodElement {
    /// Globally unique element identity.
    pub(super) id: String,
    /// Owning type-7 segment.
    segment: String,
    /// Zero-based serialized element order.
    ordinal: u32,
    /// Exact 16-byte object-type identifier.
    object_type_id: [u8; 16],
    /// Serialized object identifier.
    object_id: u32,
    /// Bytes following the common element header.
    body_byte_len: u32,
    /// SHA-256 of the bytes following the common element header.
    body_sha256: Sha256Digest,
    /// Absolute source offset of the element length.
    pub(super) source_offset: u64,
}

#[derive(Serialize)]
struct DisplayJtShapeLodElementRef<'a> {
    id: &'a str,
    segment: &'a str,
    ordinal: u32,
    object_type_id: &'a [u8; 16],
    object_base_type: u8,
    object_id: u32,
    body_byte_len: u32,
    body_sha256: &'a Sha256Digest,
    source_offset: u64,
}

impl Serialize for DisplayJtShapeLodElement {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DisplayJtShapeLodElementRef {
            id: &self.id,
            segment: &self.segment,
            ordinal: self.ordinal,
            object_type_id: &self.object_type_id,
            object_base_type: 4,
            object_id: self.object_id,
            body_byte_len: self.body_byte_len,
            body_sha256: &self.body_sha256,
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct DisplayJtShapeLodElementWire {
    id: String,
    segment: String,
    ordinal: u32,
    object_type_id: [u8; 16],
    object_base_type: u8,
    object_id: u32,
    body_byte_len: u32,
    body_sha256: Sha256Digest,
    source_offset: u64,
}

impl TryFrom<DisplayJtShapeLodElementWire> for DisplayJtShapeLodElement {
    type Error = &'static str;
    fn try_from(wire: DisplayJtShapeLodElementWire) -> Result<Self, Self::Error> {
        if wire.object_base_type != 4 {
            return Err("DisplayJtShapeLodElement.object_base_type must be 4");
        }
        Ok(Self {
            id: wire.id,
            segment: wire.segment,
            ordinal: wire.ordinal,
            object_type_id: wire.object_type_id,
            object_id: wire.object_id,
            body_byte_len: wire.body_byte_len,
            body_sha256: wire.body_sha256,
            source_offset: wire.source_offset,
        })
    }
}
#[cfg(test)]
impl From<DisplayJtShapeLodElement> for DisplayJtShapeLodElementWire {
    fn from(value: DisplayJtShapeLodElement) -> Self {
        JT_SHAPE_LOD_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: value.id,
            segment: value.segment,
            ordinal: value.ordinal,
            object_type_id: value.object_type_id,
            object_base_type: 4,
            object_id: value.object_id,
            body_byte_len: value.body_byte_len,
            body_sha256: value.body_sha256,
            source_offset: value.source_offset,
        }
    }
}

/// Fixed version and binding header of a JT 9 tri-strip shape-LOD element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DisplayJtTriStripLodHeader {
    /// Globally unique header identity.
    pub(super) id: String,
    /// Owning shape-LOD element.
    element: String,
    /// Base shape-LOD data version.
    base_version: u16,
    /// Vertex shape-LOD data version.
    vertex_version: u16,
    /// Packed vertex-channel binding mask.
    vertex_bindings: u64,
    /// Topological mesh LOD data version.
    topological_mesh_version: u16,
    /// Serialized object identifier shared by the vertex records.
    vertex_records_object_id: u32,
    /// Compressed topological-mesh representation version.
    compressed_lod_version: u16,
    /// Bytes following the fixed header.
    compressed_representation_byte_len: u32,
    /// SHA-256 of the bytes following the fixed header.
    compressed_representation_sha256: Sha256Digest,
    /// Absolute source offset of the fixed header.
    pub(super) source_offset: u64,
}

/// Decoded context-zero face-degree symbols from a JT topological mesh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DisplayJtInitialFaceDegreeSymbols {
    /// Globally unique symbol-vector identity.
    pub(super) id: String,
    /// Owning tri-strip shape-LOD element.
    element: String,
    /// Decoded symbols in topology-coder visit order.
    degrees: Vec<i32>,
    /// Complete compressed-packet byte length.
    packet_byte_len: u32,
    /// SHA-256 of the complete compressed packet.
    packet_sha256: Sha256Digest,
    /// Absolute source offset of the compressed packet.
    pub(super) source_offset: u64,
}

/// One structurally bounded compressed topology vector in a JT 9 mesh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DisplayJtTopologyPacket {
    /// Stable semantic lane name.
    role: TopologyPacketRole,
    /// Number of values represented by the packet.
    value_count: u32,
    /// Serialized compression codec identifier; zero denotes an empty vector.
    codec: u8,
    /// Complete packet length in bytes.
    byte_len: u32,
    /// Digest of the complete packet bytes.
    sha256: Sha256Digest,
    /// Mesh-representation-relative packet offset.
    representation_offset: u32,
    /// Reconstructed primal values when the packet codec is decoded.
    values: Option<Vec<i32>>,
}

/// Complete compressed-topology envelope preceding JT 9 vertex records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DisplayJtTopologyPacketSequence {
    /// Globally unique sequence identity.
    pub(super) id: String,
    /// Owning tri-strip shape-LOD element.
    element: String,
    /// Ordered topology vectors.
    packets: Vec<DisplayJtTopologyPacket>,
    /// Integrity hash serialized after the topology vectors.
    composite_hash: u32,
    /// Total topology-envelope length including the hash.
    topology_byte_len: u32,
    /// Absolute source offset of the compressed representation.
    pub(super) source_offset: u64,
}

/// Polygon connectivity reconstructed from one JT topological dual mesh.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DisplayJtPolygonMeshWire")]
pub(super) struct DisplayJtPolygonMesh {
    /// Globally unique polygon-mesh identity.
    pub(super) id: String,
    /// Owning topology packet sequence.
    topology: String,
    /// Coordinate-array header indexed by the polygons.
    coordinate_header: String,
    /// Ordered polygons with paired coordinate and attribute indices.
    polygons: Vec<Polygon>,
    /// Absolute source offset of the topology packet sequence.
    pub(super) source_offset: u64,
}

struct PolygonVertices<'a>(&'a [Polygon]);
struct PolygonAttributes<'a>(&'a [Polygon]);
struct PolygonVertexRow<'a>(&'a [(u32, Option<u32>)]);
struct PolygonAttributeRow<'a>(&'a [(u32, Option<u32>)]);

impl Serialize for PolygonVertices<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut rows = serializer.serialize_seq(Some(self.0.len()))?;
        for polygon in self.0 {
            rows.serialize_element(&PolygonVertexRow(&polygon.corners))?;
        }
        rows.end()
    }
}

impl Serialize for PolygonAttributes<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut rows = serializer.serialize_seq(Some(self.0.len()))?;
        for polygon in self.0 {
            rows.serialize_element(&PolygonAttributeRow(&polygon.corners))?;
        }
        rows.end()
    }
}

impl Serialize for PolygonVertexRow<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut values = serializer.serialize_seq(Some(self.0.len()))?;
        for (vertex, _) in self.0 {
            values.serialize_element(vertex)?;
        }
        values.end()
    }
}

impl Serialize for PolygonAttributeRow<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut values = serializer.serialize_seq(Some(self.0.len()))?;
        for (_, attribute) in self.0 {
            values.serialize_element(attribute)?;
        }
        values.end()
    }
}

impl Serialize for DisplayJtPolygonMesh {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // The group and flag columns stream below so no column buffer is built.
        use serde::ser::SerializeStruct;
        let mut wire = serializer.serialize_struct("DisplayJtPolygonMeshWire", 8)?;
        wire.serialize_field("id", &self.id)?;
        wire.serialize_field("topology", &self.topology)?;
        wire.serialize_field("coordinate_header", &self.coordinate_header)?;
        wire.serialize_field("polygons", &PolygonVertices(&self.polygons))?;
        wire.serialize_field(
            "vertex_attribute_indices",
            &PolygonAttributes(&self.polygons),
        )?;
        wire.serialize_field("polygon_groups", &PolygonGroups(&self.polygons))?;
        wire.serialize_field("polygon_flags", &PolygonFlags(&self.polygons))?;
        wire.serialize_field("source_offset", &self.source_offset)?;
        wire.end()
    }
}

struct PolygonGroups<'a>(&'a [Polygon]);
struct PolygonFlags<'a>(&'a [Polygon]);

impl Serialize for PolygonGroups<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut values = serializer.serialize_seq(Some(self.0.len()))?;
        for polygon in self.0 {
            values.serialize_element(&polygon.group)?;
        }
        values.end()
    }
}

impl Serialize for PolygonFlags<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut values = serializer.serialize_seq(Some(self.0.len()))?;
        for polygon in self.0 {
            values.serialize_element(&polygon.flags)?;
        }
        values.end()
    }
}

#[derive(Serialize, Deserialize)]
struct DisplayJtPolygonMeshWire {
    id: String,
    topology: String,
    coordinate_header: String,
    polygons: Vec<Vec<u32>>,
    vertex_attribute_indices: Vec<Vec<Option<u32>>>,
    polygon_groups: Vec<i32>,
    polygon_flags: Vec<u16>,
    source_offset: u64,
}
impl TryFrom<DisplayJtPolygonMeshWire> for DisplayJtPolygonMesh {
    type Error = &'static str;
    fn try_from(wire: DisplayJtPolygonMeshWire) -> Result<Self, Self::Error> {
        let count = wire.polygons.len();
        if wire.vertex_attribute_indices.len() != count
            || wire.polygon_groups.len() != count
            || wire.polygon_flags.len() != count
        {
            return Err("polygons/vertex_attribute_indices/polygon_groups/polygon_flags: lengths must agree");
        }
        let polygons = wire
            .polygons
            .into_iter()
            .zip(wire.vertex_attribute_indices)
            .zip(wire.polygon_groups)
            .zip(wire.polygon_flags)
            .map(|(((vertices, attributes), group), flags)| {
                if vertices.len() != attributes.len() {
                    return Err("polygon vertices and attributes: corner counts must agree");
                }
                Ok(Polygon {
                    corners: vertices.into_iter().zip(attributes).collect(),
                    group,
                    flags,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            id: wire.id,
            topology: wire.topology,
            coordinate_header: wire.coordinate_header,
            polygons,
            source_offset: wire.source_offset,
        })
    }
}
#[cfg(test)]
impl From<DisplayJtPolygonMesh> for DisplayJtPolygonMeshWire {
    fn from(value: DisplayJtPolygonMesh) -> Self {
        let mut polygons = Vec::with_capacity(value.polygons.len());
        let mut vertex_attribute_indices = Vec::with_capacity(value.polygons.len());
        let mut polygon_groups = Vec::with_capacity(value.polygons.len());
        let mut polygon_flags = Vec::with_capacity(value.polygons.len());
        for polygon in value.polygons {
            let (vertices, attributes) = polygon.corners.into_iter().unzip();
            polygons.push(vertices);
            vertex_attribute_indices.push(attributes);
            polygon_groups.push(polygon.group);
            polygon_flags.push(polygon.flags);
        }
        Self {
            id: value.id,
            topology: value.topology,
            coordinate_header: value.coordinate_header,
            polygons,
            vertex_attribute_indices,
            polygon_groups,
            polygon_flags,
            source_offset: value.source_offset,
        }
    }
}

/// Fixed header of the vertex records following a JT 9 topology envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DisplayJtCompressedVertexRecordsHeader {
    /// Globally unique header identity.
    pub(super) id: String,
    /// Owning tri-strip shape-LOD element.
    element: String,
    /// Packed vertex-channel binding mask.
    vertex_bindings: u64,
    /// Quantization bits per vertex coordinate component.
    vertex_quantization_bits: u8,
    /// Normal quantization factor.
    normal_quantization_factor: u8,
    /// Quantization bits per texture-coordinate component.
    texture_quantization_bits: u8,
    /// Quantization bits per color component.
    color_quantization_bits: u8,
    /// Number of unique topological vertices.
    topological_vertex_count: u32,
    /// Number of vertex-attribute records.
    vertex_attribute_count: u32,
    /// Remaining compressed vertex-array length.
    compressed_arrays_byte_len: u32,
    /// Digest of the remaining compressed vertex arrays.
    compressed_arrays_sha256: Sha256Digest,
    /// Absolute source offset of this header.
    pub(super) source_offset: u64,
}

/// Fixed quantization envelope of a JT 9 compressed coordinate array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct DisplayJtVertexCoordinateArrayHeader {
    /// Globally unique header identity.
    pub(super) id: String,
    /// Owning tri-strip shape-LOD element.
    element: String,
    /// Number of unique coordinate records.
    unique_vertex_count: u32,
    /// Number of coordinate components per record.
    component_count: u8,
    /// Inclusive component ranges as minimum and maximum pairs for X, Y, and Z.
    component_ranges: [QuantizedRange; 3],
    /// Quantization bits for X, Y, and Z.
    component_quantization_bits: [u8; 3],
    /// Remaining compressed component-data length.
    compressed_components_byte_len: u32,
    /// Digest of the remaining compressed component data.
    compressed_components_sha256: Sha256Digest,
    /// Absolute source offset of this header.
    pub(super) source_offset: u64,
}

/// Model-space coordinates decoded from one JT 9 vertex array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct DisplayJtVertexCoordinates {
    /// Globally unique coordinate-array identity.
    pub(super) id: String,
    /// Owning coordinate-array header.
    header: String,
    /// XYZ coordinates in the JT model's serialized metre unit.
    points_m: Vec<[FiniteBinary32; 3]>,
    /// Combined hash serialized after the component vectors.
    coordinate_hash: u32,
    /// Complete byte length of the component packets and hash.
    byte_len: u32,
    /// Absolute source offset of the first component packet.
    pub(super) source_offset: u64,
}

/// Normal vectors decoded from one JT 9 vertex-attribute array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct DisplayJtVertexNormals {
    /// Globally unique normal-array identity.
    pub(super) id: String,
    /// Owning compressed vertex-record header.
    vertex_records_header: String,
    /// Ordered unit normal vectors in attribute-record order.
    normals: Vec<[FiniteBinary32; 3]>,
    /// Combined hash serialized after the component vectors.
    normal_hash: u32,
    /// Complete byte length of the normal-array header, packets, and hash.
    byte_len: u32,
    /// Absolute source offset of the normal-array count.
    pub(super) source_offset: u64,
}

/// Colors decoded from one JT 9 vertex-attribute array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct DisplayJtVertexColors {
    /// Globally unique color-array identity.
    pub(super) id: String,
    /// Owning compressed vertex-record header.
    vertex_records_header: String,
    /// Ordered RGBA colors in vertex-attribute record order.
    colors: Vec<[FiniteBinary32; 4]>,
    /// Combined hash serialized after the component vectors.
    color_hash: u32,
    /// Complete byte length of the color-array header, packets, and hash.
    byte_len: u32,
    /// Absolute source offset of the color-array count.
    pub(super) source_offset: u64,
}

/// One decoded JT 9 vertex texture-coordinate channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct DisplayJtVertexTextureCoordinates {
    /// Globally unique channel identity.
    pub(super) id: String,
    /// Owning compressed vertex-record header.
    vertex_records_header: String,
    /// Zero-based texture-coordinate channel selected by the binding nibble.
    channel: u8,
    /// Ordered component vectors in vertex-attribute record order.
    values: Vec<Vec<FiniteBinary32>>,
    /// Combined hash serialized after the component vectors.
    texture_coordinate_hash: u32,
    /// Complete byte length of the array header, packets, and hash.
    byte_len: u32,
    /// Absolute source offset of the texture-coordinate count.
    pub(super) source_offset: u64,
}

/// One decoded JT 9 vertex-flag array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DisplayJtVertexFlags {
    /// Globally unique flag-array identity.
    pub(super) id: String,
    /// Owning compressed vertex-record header.
    vertex_records_header: String,
    /// Ordered zero-or-one flag values in vertex-attribute record order.
    values: Vec<u32>,
    /// Complete byte length of the count and compressed packet.
    byte_len: u32,
    /// Absolute source offset of the vertex-flag count.
    pub(super) source_offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JtVertexVersion {
    One,
    Two(u64),
}

impl TryFrom<(u16, Option<u64>)> for JtVertexVersion {
    type Error = &'static str;
    fn try_from((version, bindings): (u16, Option<u64>)) -> Result<Self, Self::Error> {
        match (version, bindings) {
            (1, None) => Ok(Self::One),
            (2, Some(bindings)) => Ok(Self::Two(bindings)),
            _ => Err("vertex_version/version_2_vertex_bindings: version 1 has no repeated bindings and version 2 requires them"),
        }
    }
}

impl JtVertexVersion {
    fn into_wire(self) -> (u16, Option<u64>) {
        match self {
            Self::One => (1, None),
            Self::Two(bindings) => (2, Some(bindings)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum JtMaterialVersion {
    One,
    Two(UnitBinary32),
}

impl TryFrom<(u16, Option<f32>)> for JtMaterialVersion {
    type Error = &'static str;
    fn try_from((version, reflectivity): (u16, Option<f32>)) -> Result<Self, Self::Error> {
        match (version, reflectivity) {
            (1, None) => Ok(Self::One),
            (2, Some(value)) => Ok(Self::Two(UnitBinary32::try_from(value).map_err(|_| "reflectivity: expected a finite fraction in 0..=1")?)),
            _ => Err("version/reflectivity: version 1 has no reflectivity and version 2 requires a finite fraction in 0..=1"),
        }
    }
}

impl JtMaterialVersion {
    fn into_wire(self) -> (u16, Option<f32>) {
        match self {
            Self::One => (1, None),
            Self::Two(value) => (2, Some(value.into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "Vec<f32>")]
struct JtRangeLimits(Vec<FiniteBinary32>);

impl Serialize for JtRangeLimits {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl JtRangeLimits {
    fn from_finite(values: Vec<FiniteBinary32>) -> Result<Self, &'static str> {
        if values.iter().any(|value| value.get() < 0.0)
            || values.windows(2).any(|pair| pair[0].get() >= pair[1].get())
        {
            return Err("range_limits: expected finite nonnegative strictly increasing distances");
        }
        Ok(Self(values))
    }
}

impl TryFrom<Vec<f32>> for JtRangeLimits {
    type Error = &'static str;
    fn try_from(values: Vec<f32>) -> Result<Self, Self::Error> {
        let values = values
            .into_iter()
            .map(|value| {
                FiniteBinary32::new(value).ok_or(
                    "range_limits: expected finite nonnegative strictly increasing distances",
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_finite(values)
    }
}

#[cfg(test)]
std::thread_local! {
    static JT_RANGE_LIMITS_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static JT_STRING_PROPERTY_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl From<JtRangeLimits> for Vec<f32> {
    fn from(value: JtRangeLimits) -> Self {
        JT_RANGE_LIMITS_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        value.0.into_iter().map(FiniteBinary32::get).collect()
    }
}

/// Finite bounds whose minimum does not exceed the matching maximum.
#[derive(Debug, Clone, Copy, PartialEq)]
struct JtBounds([[FiniteBinary32; 3]; 2]);

impl JtBounds {
    fn from_finite(bounds: [[FiniteBinary32; 3]; 2]) -> Option<Self> {
        bounds[0]
            .iter()
            .zip(bounds[1])
            .all(|(minimum, maximum)| minimum.get() <= maximum.get())
            .then_some(Self(bounds))
    }

    fn get(self) -> [[f32; 3]; 2] {
        self.0.map(|row| row.map(FiniteBinary32::get))
    }
}

impl TryFrom<[[f32; 3]; 2]> for JtBounds {
    type Error = &'static str;

    fn try_from(bounds: [[f32; 3]; 2]) -> Result<Self, Self::Error> {
        let [[x0, y0, z0], [x1, y1, z1]] = bounds;
        let finite =
            |value| FiniteBinary32::new(value).ok_or("bounds: expected finite ordered corners");
        let bounds = [
            [finite(x0)?, finite(y0)?, finite(z0)?],
            [finite(x1)?, finite(y1)?, finite(z1)?],
        ];
        Self::from_finite(bounds).ok_or("bounds: expected finite ordered corners")
    }
}

impl From<JtBounds> for [[f32; 3]; 2] {
    fn from(bounds: JtBounds) -> Self {
        bounds.get()
    }
}

/// A finite nonnegative JT binary32 area.
#[derive(Debug, Clone, Copy, PartialEq)]
struct JtArea(FiniteBinary32);

impl JtArea {
    fn new(value: f32) -> Option<Self> {
        FiniteBinary32::new(value)
            .filter(|_| value >= 0.0)
            .map(Self)
    }

    fn get(self) -> f32 {
        self.0.get()
    }
}

impl TryFrom<f32> for JtArea {
    type Error = &'static str;

    fn try_from(value: f32) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("area: expected finite nonnegative value")
    }
}

/// A finite JT specular exponent in the inclusive range one through 128.
#[derive(Debug, Clone, Copy, PartialEq)]
struct JtShininess(FiniteBinary32);

impl JtShininess {
    fn new(value: f32) -> Option<Self> {
        FiniteBinary32::new(value)
            .filter(|_| (1.0..=128.0).contains(&value))
            .map(Self)
    }

    fn get(self) -> f32 {
        self.0.get()
    }
}

impl TryFrom<f32> for JtShininess {
    type Error = &'static str;

    fn try_from(value: f32) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("shininess: expected finite value in [1, 128]")
    }
}

fn jt_rgba_from_wire(raw: [f32; 4]) -> Result<[UnitBinary32; 4], &'static str> {
    let [r, g, b, a] = raw;
    let unit = |value| UnitBinary32::new(value).ok_or("color: expected unit fractions");
    Ok([unit(r)?, unit(g)?, unit(b)?, unit(a)?])
}

/// A finite JT affine matrix with nonzero mutually orthogonal spatial rows.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f32; 4]; 4]", into = "[[f32; 4]; 4]")]
struct JtTransformMatrix([[FiniteBinary32; 4]; 4]);

impl JtTransformMatrix {
    fn from_finite(matrix: [[FiniteBinary32; 4]; 4]) -> Option<Self> {
        let raw = matrix.map(|row| row.map(FiniteBinary32::get));
        if raw[0][3] != 0.0 || raw[1][3] != 0.0 || raw[2][3] != 0.0 || raw[3][3] != 1.0 {
            return None;
        }
        let rows = [&raw[0][..3], &raw[1][..3], &raw[2][..3]];
        let lengths = rows.map(|row| {
            row.iter()
                .fold(0.0_f64, |length, value| length.hypot(f64::from(*value)))
        });
        if lengths
            .iter()
            .any(|length| !length.is_finite() || *length == 0.0)
        {
            return None;
        }
        for first in 0..3 {
            for second in first + 1..3 {
                let dot = rows[first]
                    .iter()
                    .zip(rows[second])
                    .map(|(left, right)| f64::from(*left) * f64::from(*right))
                    .sum::<f64>();
                if dot.abs() > 1.0e-5 * lengths[first] * lengths[second] {
                    return None;
                }
            }
        }
        Some(Self(matrix))
    }

    fn get(self) -> [[f32; 4]; 4] {
        self.0.map(|row| row.map(FiniteBinary32::get))
    }
}

impl TryFrom<[[f32; 4]; 4]> for JtTransformMatrix {
    type Error = &'static str;

    fn try_from(raw: [[f32; 4]; 4]) -> Result<Self, Self::Error> {
        let mut matrix = [[FiniteBinary32::ZERO; 4]; 4];
        for (row, values) in raw.into_iter().enumerate() {
            for (column, value) in values.into_iter().enumerate() {
                matrix[row][column] = FiniteBinary32::new(value)
                    .ok_or("matrix: expected finite affine orthogonal rows")?;
            }
        }
        Self::from_finite(matrix).ok_or("matrix: expected finite affine orthogonal rows")
    }
}

impl From<JtTransformMatrix> for [[f32; 4]; 4] {
    fn from(matrix: JtTransformMatrix) -> Self {
        matrix.get()
    }
}

/// Complete JT 9 tri-strip shape node controlling one late-loaded mesh.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DisplayJtTriStripShapeNodeWire")]
pub(super) struct DisplayJtTriStripShapeNode {
    /// Globally unique shape-node identity.
    pub(super) id: String,
    /// Owning common node-data record.
    base_node: String,
    /// Serialized node object identifier.
    object_id: u32,
    /// Reserved model-coordinate bounds.
    reserved_bounds: JtBounds,
    /// Untransformed model-coordinate bounds.
    untransformed_bounds: JtBounds,
    /// Surface area in normalized coordinate space.
    area: JtArea,
    /// Minimum and maximum vertex counts.
    vertex_count_range: [i32; 2],
    /// Minimum and maximum scene-node counts.
    node_count_range: [i32; 2],
    /// Minimum and maximum polygon counts.
    polygon_count_range: [i32; 2],
    /// Expected in-memory byte size of the late-loaded LOD.
    memory_byte_len: u32,
    /// Qualitative compression level in the inclusive range zero through one.
    compression_level: UnitBinary32,
    /// Vertex-shape data version.
    vertex_version: JtVertexVersion,
    /// Packed vertex-channel binding mask.
    vertex_bindings: u64,
    /// Quantization bits per vertex coordinate component.
    vertex_quantization_bits: u8,
    /// Normal quantization factor.
    normal_quantization_factor: u8,
    /// Quantization bits per texture-coordinate component.
    texture_quantization_bits: u8,
    /// Quantization bits per color component.
    color_quantization_bits: u8,
    /// Absolute source offset of the owning compressed envelope.
    pub(super) source_offset: u64,
}

#[derive(Serialize)]
struct DisplayJtTriStripShapeNodeRef<'a> {
    id: &'a str,
    base_node: &'a str,
    object_id: u32,
    reserved_bounds: [[f32; 3]; 2],
    untransformed_bounds: [[f32; 3]; 2],
    area: f32,
    vertex_count_range: [i32; 2],
    node_count_range: [i32; 2],
    polygon_count_range: [i32; 2],
    memory_byte_len: u32,
    compression_level: f32,
    vertex_version: u16,
    vertex_bindings: u64,
    vertex_quantization_bits: u8,
    normal_quantization_factor: u8,
    texture_quantization_bits: u8,
    color_quantization_bits: u8,
    version_2_vertex_bindings: Option<u64>,
    source_offset: u64,
}

impl Serialize for DisplayJtTriStripShapeNode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (vertex_version, version_2_vertex_bindings) = self.vertex_version.into_wire();
        DisplayJtTriStripShapeNodeRef {
            id: &self.id,
            base_node: &self.base_node,
            object_id: self.object_id,
            reserved_bounds: self.reserved_bounds.get(),
            untransformed_bounds: self.untransformed_bounds.get(),
            area: self.area.get(),
            vertex_count_range: self.vertex_count_range,
            node_count_range: self.node_count_range,
            polygon_count_range: self.polygon_count_range,
            memory_byte_len: self.memory_byte_len,
            compression_level: self.compression_level.get(),
            vertex_version,
            vertex_bindings: self.vertex_bindings,
            vertex_quantization_bits: self.vertex_quantization_bits,
            normal_quantization_factor: self.normal_quantization_factor,
            texture_quantization_bits: self.texture_quantization_bits,
            color_quantization_bits: self.color_quantization_bits,
            version_2_vertex_bindings,
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct DisplayJtTriStripShapeNodeWire {
    id: String,
    base_node: String,
    object_id: u32,
    reserved_bounds: [[f32; 3]; 2],
    untransformed_bounds: [[f32; 3]; 2],
    area: f32,
    vertex_count_range: [i32; 2],
    node_count_range: [i32; 2],
    polygon_count_range: [i32; 2],
    memory_byte_len: u32,
    compression_level: f32,
    vertex_version: u16,
    vertex_bindings: u64,
    vertex_quantization_bits: u8,
    normal_quantization_factor: u8,
    texture_quantization_bits: u8,
    color_quantization_bits: u8,
    version_2_vertex_bindings: Option<u64>,
    source_offset: u64,
}
impl TryFrom<DisplayJtTriStripShapeNodeWire> for DisplayJtTriStripShapeNode {
    type Error = &'static str;
    fn try_from(wire: DisplayJtTriStripShapeNodeWire) -> Result<Self, Self::Error> {
        let vertex_version =
            JtVertexVersion::try_from((wire.vertex_version, wire.version_2_vertex_bindings))?;
        Ok(Self {
            id: wire.id,
            base_node: wire.base_node,
            object_id: wire.object_id,
            reserved_bounds: wire.reserved_bounds.try_into()?,
            untransformed_bounds: wire.untransformed_bounds.try_into()?,
            area: wire.area.try_into()?,
            vertex_count_range: wire.vertex_count_range,
            node_count_range: wire.node_count_range,
            polygon_count_range: wire.polygon_count_range,
            memory_byte_len: wire.memory_byte_len,
            compression_level: UnitBinary32::try_from(wire.compression_level).map_err(|_| {
                "DisplayJtTriStripShapeNode.compression_level: expected a finite fraction in 0..=1"
            })?,
            vertex_version,
            vertex_bindings: wire.vertex_bindings,
            vertex_quantization_bits: wire.vertex_quantization_bits,
            normal_quantization_factor: wire.normal_quantization_factor,
            texture_quantization_bits: wire.texture_quantization_bits,
            color_quantization_bits: wire.color_quantization_bits,
            source_offset: wire.source_offset,
        })
    }
}
#[cfg(test)]
impl From<DisplayJtTriStripShapeNode> for DisplayJtTriStripShapeNodeWire {
    fn from(value: DisplayJtTriStripShapeNode) -> Self {
        let (vertex_version, version_2_vertex_bindings) = value.vertex_version.into_wire();
        Self {
            id: value.id,
            base_node: value.base_node,
            object_id: value.object_id,
            reserved_bounds: value.reserved_bounds.into(),
            untransformed_bounds: value.untransformed_bounds.into(),
            area: value.area.get(),
            vertex_count_range: value.vertex_count_range,
            node_count_range: value.node_count_range,
            polygon_count_range: value.polygon_count_range,
            memory_byte_len: value.memory_byte_len,
            compression_level: value.compression_level.into(),
            vertex_version,
            vertex_bindings: value.vertex_bindings,
            vertex_quantization_bits: value.vertex_quantization_bits,
            normal_quantization_factor: value.normal_quantization_factor,
            texture_quantization_bits: value.texture_quantization_bits,
            color_quantization_bits: value.color_quantization_bits,
            version_2_vertex_bindings,
            source_offset: value.source_offset,
        }
    }
}

/// One object element decoded from a compressed JT segment payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DisplayJtCompressedElementWire")]
pub(super) struct DisplayJtCompressedElement {
    /// Globally unique element identity.
    pub(super) id: String,
    /// Owning compressed segment.
    segment: String,
    /// Owning segment type.
    segment_type: u32,
    /// Zero-based serialized element order.
    ordinal: u32,
    /// Exact 16-byte object-type identifier.
    object_type_id: [u8; 16],
    /// Serialized object-base-type discriminator.
    object_base_type: u8,
    /// Serialized object identifier.
    object_id: u32,
    /// Bytes following the common element header.
    body_byte_len: u32,
    /// SHA-256 of the bytes following the common element header.
    body_sha256: Sha256Digest,
    /// Offset of the element length in the inflated payload.
    inflated_offset: u32,
    /// Absolute source offset of the owning compressed envelope.
    pub(super) source_offset: u64,
}

impl DisplayJtCompressedElement {
    /// Bytes following the common element header.
    fn body_byte_len(&self) -> u32 {
        self.body_byte_len
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct DisplayJtCompressedElementWire {
    id: String,
    segment: String,
    segment_type: u32,
    ordinal: u32,
    object_type_id: [u8; 16],
    object_base_type: u8,
    object_id: u32,
    body_byte_len: u32,
    body_sha256: Sha256Digest,
    inflated_offset: u32,
    source_offset: u64,
}

impl TryFrom<DisplayJtCompressedElementWire> for DisplayJtCompressedElement {
    type Error = &'static str;
    fn try_from(wire: DisplayJtCompressedElementWire) -> Result<Self, Self::Error> {
        if wire.body_byte_len.checked_add(21).is_none() {
            return Err(
                "DisplayJtCompressedElement.body_byte_len exceeds the element length range",
            );
        }
        Ok(Self {
            id: wire.id,
            segment: wire.segment,
            segment_type: wire.segment_type,
            ordinal: wire.ordinal,
            object_type_id: wire.object_type_id,
            object_base_type: wire.object_base_type,
            object_id: wire.object_id,
            body_byte_len: wire.body_byte_len,
            body_sha256: wire.body_sha256,
            inflated_offset: wire.inflated_offset,
            source_offset: wire.source_offset,
        })
    }
}

#[cfg(test)]
impl From<DisplayJtCompressedElement> for DisplayJtCompressedElementWire {
    fn from(value: DisplayJtCompressedElement) -> Self {
        JT_COMPRESSED_ELEMENT_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        Self {
            id: value.id,
            segment: value.segment,
            segment_type: value.segment_type,
            ordinal: value.ordinal,
            object_type_id: value.object_type_id,
            object_base_type: value.object_base_type,
            object_id: value.object_id,
            body_byte_len: value.body_byte_len,
            body_sha256: value.body_sha256,
            inflated_offset: value.inflated_offset,
            source_offset: value.source_offset,
        }
    }
}

/// Complete element sequence and post-marker tail of one compressed JT segment.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DisplayJtCompressedElementSequenceWire")]
pub(super) struct DisplayJtCompressedElementSequence {
    /// Globally unique sequence identity.
    pub(super) id: String,
    /// Owning compressed segment.
    segment: String,
    /// Owning segment type.
    segment_type: u32,
    /// Ordered decoded element identities.
    elements: Vec<String>,
    /// Inflated byte length through the end-object marker.
    framed_byte_len: u32,
    /// Exact bytes following the end-object marker.
    tail: Vec<u8>,
    /// SHA-256 of the exact post-marker tail.
    tail_sha256: Sha256Digest,
    /// Absolute source offset of the owning compressed envelope.
    pub(super) source_offset: u64,
}

impl DisplayJtCompressedElementSequence {
    /// Ordered decoded element identities.
    fn elements(&self) -> &[String] {
        &self.elements
    }

    /// Inflated byte length through the end-object marker.
    fn framed_byte_len(&self) -> u32 {
        self.framed_byte_len
    }
}

#[derive(Serialize)]
struct DisplayJtCompressedElementSequenceRef<'a> {
    id: &'a str,
    segment: &'a str,
    segment_type: u32,
    elements: &'a [String],
    framed_byte_len: u32,
    tail: &'a [u8],
    tail_sha256: &'a Sha256Digest,
    source_offset: u64,
}

impl Serialize for DisplayJtCompressedElementSequence {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DisplayJtCompressedElementSequenceRef {
            id: &self.id,
            segment: &self.segment,
            segment_type: self.segment_type,
            elements: &self.elements,
            framed_byte_len: self.framed_byte_len,
            tail: &self.tail,
            tail_sha256: &self.tail_sha256,
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct DisplayJtCompressedElementSequenceWire {
    id: String,
    segment: String,
    segment_type: u32,
    elements: Vec<String>,
    framed_byte_len: u32,
    tail: Vec<u8>,
    tail_sha256: Sha256Digest,
    source_offset: u64,
}

impl TryFrom<DisplayJtCompressedElementSequenceWire> for DisplayJtCompressedElementSequence {
    type Error = &'static str;
    fn try_from(wire: DisplayJtCompressedElementSequenceWire) -> Result<Self, Self::Error> {
        if wire.tail_sha256 != Sha256Digest::digest(&wire.tail) {
            return Err("DisplayJtCompressedElementSequence.tail_sha256 disagrees with tail");
        }
        let minimum = u64::try_from(wire.elements.len())
            .ok()
            .and_then(|count| count.checked_mul(25))
            .and_then(|bytes| bytes.checked_add(20));
        if minimum.is_none_or(|minimum| minimum > u64::from(wire.framed_byte_len)) {
            return Err("DisplayJtCompressedElementSequence.framed_byte_len cannot contain its elements and end marker");
        }
        Ok(Self {
            id: wire.id,
            segment: wire.segment,
            segment_type: wire.segment_type,
            elements: wire.elements,
            framed_byte_len: wire.framed_byte_len,
            tail: wire.tail,
            tail_sha256: wire.tail_sha256,
            source_offset: wire.source_offset,
        })
    }
}

#[cfg(test)]
std::thread_local! {
    static JT_COMPRESSED_SEQUENCE_INTO_WIRE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
impl From<DisplayJtCompressedElementSequence> for DisplayJtCompressedElementSequenceWire {
    fn from(value: DisplayJtCompressedElementSequence) -> Self {
        JT_COMPRESSED_SEQUENCE_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        let tail_sha256 = Sha256Digest::digest(&value.tail);
        Self {
            id: value.id,
            segment: value.segment,
            segment_type: value.segment_type,
            elements: value.elements,
            framed_byte_len: value.framed_byte_len,
            tail: value.tail,
            tail_sha256,
            source_offset: value.source_offset,
        }
    }
}

/// One UTF-16 string property atom in a type-31 JT segment.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "DisplayJtStringPropertyAtomWire")]
pub(super) struct DisplayJtStringPropertyAtom {
    /// Globally unique property-atom identity.
    pub(super) id: String,
    /// Owning compressed element.
    element: String,
    /// Serialized object identifier.
    object_id: u32,
    /// Decoded string value.
    value: String,
    /// Absolute source offset of the owning compressed envelope.
    pub(super) source_offset: u64,
}

struct Utf16CodeUnits<'a>(&'a str);

impl Serialize for Utf16CodeUnits<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.encode_utf16().count()))?;
        for unit in self.0.encode_utf16() {
            sequence.serialize_element(&unit)?;
        }
        sequence.end()
    }
}

#[derive(Serialize)]
struct DisplayJtStringPropertyAtomRef<'a> {
    id: &'a str,
    element: &'a str,
    object_id: u32,
    code_units: Utf16CodeUnits<'a>,
    value: &'a str,
    source_offset: u64,
}

impl Serialize for DisplayJtStringPropertyAtom {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DisplayJtStringPropertyAtomRef {
            id: &self.id,
            element: &self.element,
            object_id: self.object_id,
            code_units: Utf16CodeUnits(&self.value),
            value: &self.value,
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(Serialize))]
struct DisplayJtStringPropertyAtomWire {
    id: String,
    element: String,
    object_id: u32,
    code_units: Vec<u16>,
    value: String,
    source_offset: u64,
}

#[cfg(test)]
impl From<DisplayJtStringPropertyAtom> for DisplayJtStringPropertyAtomWire {
    fn from(value: DisplayJtStringPropertyAtom) -> Self {
        JT_STRING_PROPERTY_INTO_WIRE_COUNT.with(|count| count.set(count.get() + 1));
        let code_units = value.value.encode_utf16().collect();
        Self {
            id: value.id,
            element: value.element,
            object_id: value.object_id,
            code_units,
            value: value.value,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<DisplayJtStringPropertyAtomWire> for DisplayJtStringPropertyAtom {
    type Error = &'static str;
    fn try_from(wire: DisplayJtStringPropertyAtomWire) -> Result<Self, Self::Error> {
        if !wire
            .value
            .encode_utf16()
            .eq(wire.code_units.iter().copied())
        {
            return Err("DisplayJtStringPropertyAtom.code_units disagrees with value");
        }
        Ok(Self {
            id: wire.id,
            element: wire.element,
            object_id: wire.object_id,
            value: wire.value,
            source_offset: wire.source_offset,
        })
    }
}

/// Property-table link from a logical shape node to a late-loaded LOD segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DisplayJtShapeLodBinding {
    /// Globally unique binding identity.
    pub(super) id: String,
    /// Owning type-1 logical scene-graph segment.
    scene_segment: String,
    /// Serialized property-table version.
    table_version: u16,
    /// Shape-node object identifier owning the property pair.
    shape_node_object_id: u32,
    /// String-property object identifier used as the key.
    key_object_id: u32,
    /// Exact decoded property key.
    key: String,
    /// Late-loaded-property object identifier used as the value.
    value_object_id: u32,
    /// Base-property state flags.
    state_flags: u32,
    /// Late-loaded-property version.
    property_version: u16,
    /// Resolved type-7 shape-LOD segment.
    shape_segment: String,
    /// Serialized payload object identifier within the shape-LOD segment.
    payload_object_id: u32,
    /// Serialized positive late-loaded-property reserved value.
    reserved_value: u32,
    /// Absolute source offset of the owning compressed envelope.
    pub(super) source_offset: u64,
}

/// Common node-data header carried by one type-1 JT element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DisplayJtBaseNodeData {
    /// Globally unique node-data identity.
    pub(super) id: String,
    /// Owning compressed element.
    element: String,
    /// Exact 16-byte object-type identifier of the owning element.
    object_type_id: [u8; 16],
    /// Serialized node object identifier.
    object_id: u32,
    /// Common node-data version.
    version: u16,
    /// Serialized node flags.
    flags: u32,
    /// Ordered attribute object identifiers.
    attribute_object_ids: Vec<u32>,
    /// Byte length after the common node-data header.
    family_data_byte_len: u32,
    /// SHA-256 of the bytes after the common node-data header.
    family_data_sha256: Sha256Digest,
    /// Absolute source offset of the owning compressed envelope.
    pub(super) source_offset: u64,
}

/// Complete JT 9 instance node referencing one shared logical scene node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DisplayJtInstanceNode {
    /// Globally unique instance-node identity.
    pub(super) id: String,
    /// Owning common node-data record.
    base_node: String,
    /// Serialized instance-node object identifier.
    object_id: u32,
    /// Instance-node data version.
    version: u16,
    /// Referenced child node object identifier.
    child_object_id: u32,
    /// Absolute source offset of the owning compressed envelope.
    pub(super) source_offset: u64,
}

/// Common JT 9 group-node data carried by every group-derived scene node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DisplayJtGroupNodeData {
    /// Globally unique group-data identity.
    pub(super) id: String,
    /// Owning common node-data record.
    base_node: String,
    /// Serialized group-derived node object identifier.
    object_id: u32,
    /// Group-node data version.
    version: u16,
    /// Ordered child node object identifiers.
    child_object_ids: Vec<u32>,
    /// Byte length after the common group-node data.
    family_data_byte_len: u32,
    /// SHA-256 of the bytes after the common group-node data.
    family_data_sha256: Sha256Digest,
    /// Absolute source offset of the owning compressed envelope.
    pub(super) source_offset: u64,
}

/// One JT geometric-transform attribute attached to logical scene nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct DisplayJtGeometricTransformAttribute {
    /// Globally unique transform-attribute identity.
    pub(super) id: String,
    /// Owning compressed logical scene-graph element.
    element: String,
    /// Serialized attribute object identifier referenced by nodes.
    object_id: u32,
    /// Base-attribute state flags.
    state_flags: u8,
    /// Base-attribute field-inhibit flags.
    field_inhibit_flags: u32,
    /// Sparse-matrix stored-values mask in row-major bit order.
    stored_values_mask: u16,
    /// Complete row-major local-to-parent homogeneous matrix.
    matrix: JtTransformMatrix,
    /// Absolute source offset of the owning compressed envelope.
    pub(super) source_offset: u64,
}

/// One JT material attribute attached to logical scene nodes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DisplayJtMaterialAttributeWire")]
pub(super) struct DisplayJtMaterialAttribute {
    /// Globally unique material-attribute identity.
    pub(super) id: String,
    /// Owning compressed logical scene-graph element.
    element: String,
    /// Serialized attribute object identifier referenced by nodes.
    object_id: u32,
    /// Base-attribute state flags.
    state_flags: u8,
    /// Base-attribute field-inhibit flags.
    field_inhibit_flags: u32,
    /// Material-record version.
    version: JtMaterialVersion,
    /// Material blending and vertex-color override flags.
    data_flags: u16,
    /// Ambient RGBA components.
    ambient: [UnitBinary32; 4],
    /// Diffuse RGBA components.
    diffuse: [UnitBinary32; 4],
    /// Specular RGBA components.
    specular: [UnitBinary32; 4],
    /// Emission RGBA components.
    emission: [UnitBinary32; 4],
    /// Specular exponent in the inclusive range 1 through 128.
    shininess: JtShininess,
    /// Absolute source offset of the owning compressed envelope.
    pub(super) source_offset: u64,
}

#[derive(Serialize)]
struct DisplayJtMaterialAttributeRef<'a> {
    id: &'a str,
    element: &'a str,
    object_id: u32,
    state_flags: u8,
    field_inhibit_flags: u32,
    version: u16,
    data_flags: u16,
    ambient: [f32; 4],
    diffuse: [f32; 4],
    specular: [f32; 4],
    emission: [f32; 4],
    shininess: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    reflectivity: Option<f32>,
    source_offset: u64,
}

impl Serialize for DisplayJtMaterialAttribute {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (version, reflectivity) = self.version.into_wire();
        DisplayJtMaterialAttributeRef {
            id: &self.id,
            element: &self.element,
            object_id: self.object_id,
            state_flags: self.state_flags,
            field_inhibit_flags: self.field_inhibit_flags,
            version,
            data_flags: self.data_flags,
            ambient: self.ambient.map(UnitBinary32::get),
            diffuse: self.diffuse.map(UnitBinary32::get),
            specular: self.specular.map(UnitBinary32::get),
            emission: self.emission.map(UnitBinary32::get),
            shininess: self.shininess.get(),
            reflectivity,
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct DisplayJtMaterialAttributeWire {
    id: String,
    element: String,
    object_id: u32,
    state_flags: u8,
    field_inhibit_flags: u32,
    version: u16,
    data_flags: u16,
    ambient: [f32; 4],
    diffuse: [f32; 4],
    specular: [f32; 4],
    emission: [f32; 4],
    shininess: f32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_reflectivity"
    )]
    reflectivity: Option<f32>,
    source_offset: u64,
}
impl TryFrom<DisplayJtMaterialAttributeWire> for DisplayJtMaterialAttribute {
    type Error = &'static str;
    fn try_from(wire: DisplayJtMaterialAttributeWire) -> Result<Self, Self::Error> {
        let version = JtMaterialVersion::try_from((wire.version, wire.reflectivity))?;
        Ok(Self {
            id: wire.id,
            element: wire.element,
            object_id: wire.object_id,
            state_flags: wire.state_flags,
            field_inhibit_flags: wire.field_inhibit_flags,
            version,
            data_flags: wire.data_flags,
            ambient: jt_rgba_from_wire(wire.ambient)?,
            diffuse: jt_rgba_from_wire(wire.diffuse)?,
            specular: jt_rgba_from_wire(wire.specular)?,
            emission: jt_rgba_from_wire(wire.emission)?,
            shininess: wire.shininess.try_into()?,
            source_offset: wire.source_offset,
        })
    }
}
#[cfg(test)]
impl From<DisplayJtMaterialAttribute> for DisplayJtMaterialAttributeWire {
    fn from(value: DisplayJtMaterialAttribute) -> Self {
        let (version, reflectivity) = value.version.into_wire();
        Self {
            id: value.id,
            element: value.element,
            object_id: value.object_id,
            state_flags: value.state_flags,
            field_inhibit_flags: value.field_inhibit_flags,
            version,
            data_flags: value.data_flags,
            ambient: value.ambient.map(UnitBinary32::get),
            diffuse: value.diffuse.map(UnitBinary32::get),
            specular: value.specular.map(UnitBinary32::get),
            emission: value.emission.map(UnitBinary32::get),
            shininess: value.shininess.get(),
            reflectivity,
            source_offset: value.source_offset,
        }
    }
}

/// Extra partition-node bounds selected by flag bit zero.
#[derive(Debug, Clone, PartialEq)]
enum DisplayJtPartitionBounds {
    /// Reserved bounds when partition flag bit zero is clear.
    Reserved(JtBounds),
    /// Untransformed bounds when partition flag bit zero is set.
    Untransformed(JtBounds),
}

/// Complete JT 9 partition node linking an LSG branch to a partition file.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "DisplayJtPartitionNodeWire")]
pub(super) struct DisplayJtPartitionNode {
    /// Globally unique partition-node identity.
    pub(super) id: String,
    /// Owning common node-data record.
    base_node: String,
    /// Serialized node object identifier.
    object_id: u32,
    /// Group-node data version.
    group_version: u16,
    /// Ordered child node object identifiers.
    child_object_ids: Vec<u32>,
    /// Decoded partition filename.
    file_name: String,
    /// Transformed axis-aligned bounds as minimum and maximum XYZ corners.
    transformed_bounds: JtBounds,
    /// Total descendant surface area in normalized coordinate space.
    area: JtArea,
    /// Minimum and maximum descendant vertex counts.
    vertex_count_range: [i32; 2],
    /// Minimum and maximum descendant node counts.
    node_count_range: [i32; 2],
    /// Minimum and maximum descendant polygon counts.
    polygon_count_range: [i32; 2],
    /// Extra bounds selected by partition flag bit zero.
    bounds: DisplayJtPartitionBounds,
    /// Absolute source offset of the owning compressed envelope.
    pub(super) source_offset: u64,
}

#[derive(Serialize)]
struct DisplayJtPartitionNodeRef<'a> {
    id: &'a str,
    base_node: &'a str,
    object_id: u32,
    group_version: u16,
    child_object_ids: &'a [u32],
    partition_flags: u32,
    file_name_code_units: Utf16CodeUnits<'a>,
    file_name: &'a str,
    transformed_bounds: [[f32; 3]; 2],
    area: f32,
    vertex_count_range: [i32; 2],
    node_count_range: [i32; 2],
    polygon_count_range: [i32; 2],
    untransformed_bounds: Option<[[f32; 3]; 2]>,
    reserved_bounds: Option<[[f32; 3]; 2]>,
    source_offset: u64,
}

impl Serialize for DisplayJtPartitionNode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (partition_flags, untransformed_bounds, reserved_bounds) = match self.bounds {
            DisplayJtPartitionBounds::Reserved(bounds) => (0, None, Some(bounds.get())),
            DisplayJtPartitionBounds::Untransformed(bounds) => (1, Some(bounds.get()), None),
        };
        DisplayJtPartitionNodeRef {
            id: &self.id,
            base_node: &self.base_node,
            object_id: self.object_id,
            group_version: self.group_version,
            child_object_ids: &self.child_object_ids,
            partition_flags,
            file_name_code_units: Utf16CodeUnits(&self.file_name),
            file_name: &self.file_name,
            transformed_bounds: self.transformed_bounds.get(),
            area: self.area.get(),
            vertex_count_range: self.vertex_count_range,
            node_count_range: self.node_count_range,
            polygon_count_range: self.polygon_count_range,
            untransformed_bounds,
            reserved_bounds,
            source_offset: self.source_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct DisplayJtPartitionNodeWire {
    id: String,
    base_node: String,
    object_id: u32,
    group_version: u16,
    child_object_ids: Vec<u32>,
    partition_flags: u32,
    file_name_code_units: Vec<u16>,
    file_name: String,
    transformed_bounds: [[f32; 3]; 2],
    area: f32,
    vertex_count_range: [i32; 2],
    node_count_range: [i32; 2],
    polygon_count_range: [i32; 2],
    untransformed_bounds: Option<[[f32; 3]; 2]>,
    reserved_bounds: Option<[[f32; 3]; 2]>,
    source_offset: u64,
}

#[cfg(test)]
impl From<DisplayJtPartitionNode> for DisplayJtPartitionNodeWire {
    fn from(value: DisplayJtPartitionNode) -> Self {
        let (partition_flags, untransformed_bounds, reserved_bounds) = match value.bounds {
            DisplayJtPartitionBounds::Reserved(bounds) => (0, None, Some(bounds.into())),
            DisplayJtPartitionBounds::Untransformed(bounds) => (1, Some(bounds.into()), None),
        };
        Self {
            id: value.id,
            base_node: value.base_node,
            object_id: value.object_id,
            group_version: value.group_version,
            child_object_ids: value.child_object_ids,
            partition_flags,
            file_name_code_units: value.file_name.encode_utf16().collect(),
            file_name: value.file_name,
            transformed_bounds: value.transformed_bounds.into(),
            area: value.area.get(),
            vertex_count_range: value.vertex_count_range,
            node_count_range: value.node_count_range,
            polygon_count_range: value.polygon_count_range,
            untransformed_bounds,
            reserved_bounds,
            source_offset: value.source_offset,
        }
    }
}

impl TryFrom<DisplayJtPartitionNodeWire> for DisplayJtPartitionNode {
    type Error = String;

    fn try_from(wire: DisplayJtPartitionNodeWire) -> Result<Self, Self::Error> {
        if !wire
            .file_name
            .encode_utf16()
            .eq(wire.file_name_code_units.iter().copied())
        {
            return Err(
                "DisplayJtPartitionNode.file_name_code_units disagrees with file_name".into(),
            );
        }
        let bounds = match (
            wire.partition_flags,
            wire.untransformed_bounds,
            wire.reserved_bounds,
        ) {
            (0, None, Some(bounds)) => DisplayJtPartitionBounds::Reserved(bounds.try_into()?),
            (1, Some(bounds), None) => {
                DisplayJtPartitionBounds::Untransformed(bounds.try_into()?)
            }
            _ => {
                return Err(
                    "JT partition bounds are reserved when flag bit 0 is clear and untransformed when it is set"
                        .to_owned(),
                )
            }
        };
        Ok(Self {
            id: wire.id,
            base_node: wire.base_node,
            object_id: wire.object_id,
            group_version: wire.group_version,
            child_object_ids: wire.child_object_ids,
            file_name: wire.file_name,
            transformed_bounds: wire.transformed_bounds.try_into()?,
            area: wire.area.try_into()?,
            vertex_count_range: wire.vertex_count_range,
            node_count_range: wire.node_count_range,
            polygon_count_range: wire.polygon_count_range,
            bounds,
            source_offset: wire.source_offset,
        })
    }
}

/// Complete JT 9 range-LOD node selecting among ordered child nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct DisplayJtRangeLodNode {
    /// Globally unique range-LOD-node identity.
    pub(super) id: String,
    /// Owning common node-data record.
    base_node: String,
    /// Serialized node object identifier.
    object_id: u32,
    /// Group-node data version.
    group_version: u16,
    /// Ordered alternate-representation child identifiers.
    child_object_ids: Vec<u32>,
    /// LOD-node data version.
    lod_version: u16,
    /// Reserved finite floating-point vector.
    reserved_values: Vec<FiniteBinary32>,
    /// Reserved signed integer.
    reserved_value: i32,
    /// Range-LOD data version.
    range_version: u16,
    /// Strictly increasing nonnegative eye-distance limits.
    range_limits: JtRangeLimits,
    /// Model-coordinate centre for range selection.
    center: [FiniteBinary32; 3],
    /// Absolute source offset of the owning compressed envelope.
    pub(super) source_offset: u64,
}

#[derive(Debug)]
struct ParsedJtElement<'a> {
    offset: usize,
    object_type_id: [u8; 16],
    object_id: u32,
    object_base_type: u8,
    body: &'a [u8],
}

fn parse_jt_element_sequence<'a>(
    ctx: &DecodeContext<'_>,
    payload: &'a [u8],
) -> Result<Option<(Vec<ParsedJtElement<'a>>, usize)>, CodecError> {
    const END_OBJECT_TYPE: [u8; 16] = [0xff; 16];
    let mut elements = Vec::new();
    let mut view = View::over_retained(payload);
    loop {
        ctx.charge_work(1, "scan DisplayJT element")?;
        let cursor = view.position();
        let Some(element_byte_len) = view.u32_le() else {
            return Ok(None);
        };
        let Ok(element_len) = usize::try_from(element_byte_len) else {
            return Ok(None);
        };
        let Some(element) = view.take(element_len) else {
            return Ok(None);
        };
        if element_byte_len == 16 && element == END_OBJECT_TYPE {
            return Ok(Some((elements, view.position())));
        }
        let Some(object_type_id) = element.get(..16).and_then(|bytes| bytes.try_into().ok()) else {
            return Ok(None);
        };
        let Some(&object_base_type) = element.get(16) else {
            return Ok(None);
        };
        let Some(object_id) = View::u32_le_at(element, 17) else {
            return Ok(None);
        };
        let Some(body) = element.get(21..) else {
            return Ok(None);
        };
        ctx.reserve_retained_vec(&mut elements, 1, "store DisplayJT element")?;
        elements.push(ParsedJtElement {
            offset: cursor,
            object_type_id,
            object_id,
            object_base_type,
            body,
        });
    }
}

fn parse_jt_string_property_atom_body(
    ctx: &DecodeContext<'_>,
    body: &[u8],
) -> Result<Option<String>, CodecError> {
    const PREFIX: [u8; 8] = [1, 0, 0, 0, 0, 0x40, 1, 0];
    if body.get(..8) != Some(PREFIX.as_slice()) {
        return Ok(None);
    }
    let mut view = View::over_retained(body);
    if view.seek(8).is_none() {
        return Ok(None);
    }
    let Some(count) = view.u32_le().and_then(|count| usize::try_from(count).ok()) else {
        return Ok(None);
    };
    let Some(unit_bytes) = count.checked_mul(2) else {
        return Ok(None);
    };
    if view.remaining() != unit_bytes {
        return Ok(None);
    }
    let Some(raw) = body.get(view.position()..) else {
        return Ok(None);
    };
    match ctx.utf16le_text(raw, count, false, "retain DisplayJT string property") {
        Ok(value) => Ok(Some(value)),
        Err(CodecError::Malformed(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

fn parse_jt9_tri_strip_lod_header(body: &[u8]) -> Option<(u64, u16, u32, u16, &[u8])> {
    let mut view = View::over_retained(body);
    let base_version = view.u16_le()?;
    let vertex_version = view.u16_le()?;
    let vertex_bindings = view.u64_le()?;
    let topological_mesh_version = view.u16_le()?;
    let vertex_records_object_id = view.u32_le()?;
    let compressed_lod_version = view.u16_le()?;
    if base_version != 1 || vertex_version != 1 || !matches!(topological_mesh_version, 1 | 2) {
        return None;
    }
    if !matches!(compressed_lod_version, 1 | 2) {
        return None;
    }
    Some((
        vertex_bindings,
        topological_mesh_version,
        vertex_records_object_id,
        compressed_lod_version,
        body.get(view.position()..)?,
    ))
}

fn jt9_topology_high_degree_lane_count(
    ctx: &DecodeContext<'_>,
    representation: &[u8],
    expected_vertex_bindings: u64,
) -> Result<Option<usize>, CodecError> {
    let result =
        jt9_topology_high_degree_lane_count_inner(ctx, representation, expected_vertex_bindings)?;
    ctx.charge_work(0, "complete JT topology lane scan")?;
    Ok(result)
}

fn jt9_topology_high_degree_lane_count_inner(
    ctx: &DecodeContext<'_>,
    representation: &[u8],
    expected_vertex_bindings: u64,
) -> Result<Option<usize>, CodecError> {
    const PREFIX_PACKET_COUNT: usize = 21;
    let mut prefix_end = 0usize;
    for _ in 0..PREFIX_PACKET_COUNT {
        let Some(bytes) = representation.get(prefix_end..) else {
            return Ok(None);
        };
        let Some((_, _, byte_len)) = crate::jt::frame_int32_cdp2(ctx, bytes, 0)? else {
            return Ok(None);
        };
        let Some(end) = prefix_end.checked_add(byte_len) else {
            return Ok(None);
        };
        prefix_end = end;
    }
    let mut match_count = 0usize;
    let mut matched_lane_count = 0usize;
    let mut cursor = prefix_end;
    let mut lane_count = 1usize;
    while cursor < representation.len() {
        let Some(bytes) = representation.get(cursor..) else {
            break;
        };
        let Some((_, _, byte_len)) = crate::jt::frame_int32_cdp2(ctx, bytes, 0)? else {
            break;
        };
        let Some(end) = cursor.checked_add(byte_len) else {
            return Ok(None);
        };
        cursor = end;
        let mut candidate_end = cursor;
        let mut split_packets_valid = true;
        for _ in 0..2 {
            let Some(bytes) = representation.get(candidate_end..) else {
                split_packets_valid = false;
                break;
            };
            let Some((_, _, byte_len)) = crate::jt::frame_int32_cdp2(ctx, bytes, 0)? else {
                split_packets_valid = false;
                break;
            };
            let Some(end) = candidate_end.checked_add(byte_len) else {
                return Ok(None);
            };
            candidate_end = end;
        }
        if !split_packets_valid {
            continue;
        }
        let Some(header_end) = candidate_end.checked_add(20) else {
            continue;
        };
        let Some(envelope) = representation.get(candidate_end..header_end) else {
            continue;
        };
        let Some(bindings) = View::u64_le_at(envelope, 4) else {
            return Ok(None);
        };
        let quantization = &envelope[12..16];
        let Some(topological_vertex_count) = View::u32_le_at(envelope, 16) else {
            return Ok(None);
        };
        let vertex_attribute_count = if topological_vertex_count == 0 {
            0
        } else {
            let Some(count) = View::u32_le_at(representation, header_end) else {
                continue;
            };
            count
        };
        if bindings == expected_vertex_bindings
            && quantization[0] <= 24
            && quantization[1] <= 13
            && quantization[2] <= 24
            && quantization[3] <= 24
            && i32::try_from(topological_vertex_count).is_ok()
            && i32::try_from(vertex_attribute_count).is_ok()
        {
            match_count += 1;
            matched_lane_count = lane_count;
        }
        let Some(next_lane_count) = lane_count.checked_add(1) else {
            return Ok(None);
        };
        lane_count = next_lane_count;
    }
    Ok((match_count == 1).then_some(matched_lane_count))
}

fn parse_jt_base_node_body(body: &[u8], format_major: u16) -> Option<(u16, u32, Vec<u32>, &[u8])> {
    let (version, flags_offset, count_offset, attributes_offset): (u16, usize, usize, usize) =
        if format_major < 10 {
            (View::u16_le_at(body, 0)?, 2, 6, 10)
        } else {
            (u16::from(*body.first()?), 1, 5, 9)
        };
    let flags = View::u32_le_at(body, flags_offset)?;
    let attribute_count = View::u32_le_at(body, count_offset)?;
    let mut view = View::over_retained(body);
    view.seek(attributes_offset)?;
    let attribute_object_ids = view.read_counted(u64::from(attribute_count), 4, View::u32_le)?;
    Some((
        version,
        flags,
        attribute_object_ids,
        body.get(view.position()..)?,
    ))
}

type AdmittedJtTail<'a, 'ctx> = Option<(&'a [u8], Option<ScopedReservation<'ctx>>)>;

#[derive(Debug)]
enum JtOptionalReservation<'a> {
    Invalid,
    Admitted(ScopedReservation<'a>),
}

fn admit_jt_counted_u32<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    bytes: &'a [u8],
    count_offset: usize,
    values_offset: usize,
    operation: &'static str,
    retained: bool,
) -> Result<AdmittedJtTail<'a, 'ctx>, CodecError> {
    let Some(count) = View::u32_le_at(bytes, count_offset) else {
        return Ok(None);
    };
    let Some(byte_len) = usize::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(4))
    else {
        return Ok(None);
    };
    let Some(end) = values_offset.checked_add(byte_len) else {
        return Ok(None);
    };
    let Some(tail) = bytes.get(end..) else {
        return Ok(None);
    };
    ctx.charge_collection_items(u64::from(count), operation)?;
    let reservation = if retained {
        ctx.charge_retained(u64::from(count) * 4, operation)?;
        None
    } else {
        Some(ctx.reserve_scoped(u64::from(count) * 4, operation)?)
    };
    Ok(Some((tail, reservation)))
}

fn admit_jt_base_body<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    body: &'a [u8],
    major: u16,
    retained: bool,
) -> Result<AdmittedJtTail<'a, 'ctx>, CodecError> {
    let (count_offset, values_offset) = if major < 10 { (6, 10) } else { (5, 9) };
    admit_jt_counted_u32(
        ctx,
        body,
        count_offset,
        values_offset,
        "decode DisplayJT base node attributes",
        retained,
    )
}

fn admit_jt_group_body<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    body: &'a [u8],
) -> Result<AdmittedJtTail<'a, 'ctx>, CodecError> {
    let Some((family, base_reservation)) = admit_jt_base_body(ctx, body, 9, false)? else {
        return Ok(None);
    };
    let Some((family, _)) =
        admit_jt_counted_u32(ctx, family, 2, 6, "decode DisplayJT group children", true)?
    else {
        return Ok(None);
    };
    Ok(Some((family, base_reservation)))
}

fn jt_f32_vector_tail(bytes: &[u8]) -> Option<(&[u8], u64)> {
    let count = View::u32_le_at(bytes, 0)?;
    let byte_len = usize::try_from(count).ok()?.checked_mul(4)?;
    let end = 4usize.checked_add(byte_len)?;
    Some((bytes.get(end..)?, u64::from(count)))
}

fn admit_jt_range_vectors<'a>(
    ctx: &'a DecodeContext<'_>,
    family: &[u8],
) -> Result<JtOptionalReservation<'a>, CodecError> {
    let Some(first) = family.get(2..) else {
        return Ok(JtOptionalReservation::Invalid);
    };
    let Some((after_first, first_count)) = jt_f32_vector_tail(first) else {
        return Ok(JtOptionalReservation::Invalid);
    };
    let Some(second) = after_first.get(6..) else {
        return Ok(JtOptionalReservation::Invalid);
    };
    let Some((_, second_count)) = jt_f32_vector_tail(second) else {
        return Ok(JtOptionalReservation::Invalid);
    };
    let total = first_count
        .checked_add(second_count)
        .ok_or_else(|| ctx.refuse_codec_limit("decode DisplayJT range values", 0, u64::MAX))?;
    ctx.charge_work(total, "decode DisplayJT range values")?;
    ctx.charge_collection_items(
        total
            .checked_mul(2)
            .ok_or_else(|| ctx.refuse_codec_limit("decode DisplayJT range values", 0, u64::MAX))?,
        "decode DisplayJT range values",
    )?;
    ctx.charge_retained(
        total
            .checked_mul(4)
            .ok_or_else(|| ctx.refuse_codec_limit("retain DisplayJT range values", 0, u64::MAX))?,
        "retain DisplayJT range values",
    )?;
    let reservation = ctx.reserve_scoped(
        first_count.max(second_count) * 4,
        "decode DisplayJT range values",
    )?;
    Ok(JtOptionalReservation::Admitted(reservation))
}

fn parse_jt9_instance_node_body(body: &[u8]) -> Option<(u16, u32)> {
    let (_, _, _, family) = parse_jt_base_node_body(body, 9)?;
    let version = View::u16_le_at(family, 0)?;
    let child_object_id = View::u32_le_at(family, 2)?;
    (version == 1 && family.len() == 6).then_some((version, child_object_id))
}

struct ParsedJtTriStripShapeNode {
    reserved_bounds: JtBounds,
    untransformed_bounds: JtBounds,
    area: JtArea,
    vertex_count_range: [i32; 2],
    node_count_range: [i32; 2],
    polygon_count_range: [i32; 2],
    memory_byte_len: u32,
    compression_level: UnitBinary32,
    vertex_version: JtVertexVersion,
    vertex_bindings: u64,
    vertex_quantization_bits: u8,
    normal_quantization_factor: u8,
    texture_quantization_bits: u8,
    color_quantization_bits: u8,
}

fn parse_jt9_tri_strip_shape_node_body(body: &[u8]) -> Option<ParsedJtTriStripShapeNode> {
    let (_, _, _, family) = parse_jt_base_node_body(body, 9)?;
    if family.len() < jt_family::LEN || View::u16_le_at(family, jt_family::SHAPE_VERSION)? != 1 {
        return None;
    }
    let f32_at = |offset: usize| FiniteBinary32::new(View::f32_le_at(family, offset)?);
    let bounds_at = |offset: usize| {
        let bounds = [
            [f32_at(offset)?, f32_at(offset + 4)?, f32_at(offset + 8)?],
            [
                f32_at(offset + 12)?,
                f32_at(offset + 16)?,
                f32_at(offset + 20)?,
            ],
        ];
        JtBounds::from_finite(bounds)
    };
    let range_at = |offset: usize| {
        let range = [
            View::i32_le_at(family, offset)?,
            View::i32_le_at(family, offset + 4)?,
        ];
        (range[0] >= 0 && range[0] <= range[1]).then_some(range)
    };
    let compression_level =
        UnitBinary32::try_from(View::f32_le_at(family, jt_family::COMPRESSION_LEVEL)?).ok()?;
    let area = JtArea::new(View::f32_le_at(family, jt_family::AREA)?)?;
    let vertex_version = View::u16_le_at(family, jt_family::VERTEX_VERSION)?;
    if !matches!(vertex_version, 1 | 2) {
        return None;
    }
    let expected_len = if vertex_version == 1 {
        jt_family::LEN
    } else {
        108
    };
    if family.len() != expected_len {
        return None;
    }
    let vertex_bindings = View::u64_le_at(family, jt_family::VERTEX_BINDINGS)?;
    let vertex_quantization_bits = family[jt_family::VERTEX_QUANTIZATION_BITS];
    let normal_quantization_factor = family[jt_family::NORMAL_QUANTIZATION_FACTOR];
    let texture_quantization_bits = family[jt_family::TEXTURE_QUANTIZATION_BITS];
    let color_quantization_bits = family[jt_family::COLOR_QUANTIZATION_BITS];
    let vertex_version = match vertex_version {
        1 => JtVertexVersion::One,
        2 => JtVertexVersion::Two(assemble_u64_le(
            View::over_retained(family.get(jt_family::LEN..)?).array::<8>()?,
        )),
        _ => return None,
    };
    if vertex_quantization_bits > 24
        || normal_quantization_factor > 13
        || texture_quantization_bits > 24
        || color_quantization_bits > 24
    {
        return None;
    }
    Some(ParsedJtTriStripShapeNode {
        reserved_bounds: bounds_at(jt_family::RESERVED_BOUNDS)?,
        untransformed_bounds: bounds_at(jt_family::UNTRANSFORMED_BOUNDS)?,
        area,
        vertex_count_range: range_at(jt_family::VERTEX_COUNT_RANGE)?,
        node_count_range: range_at(jt_family::NODE_COUNT_RANGE)?,
        polygon_count_range: range_at(jt_family::POLYGON_COUNT_RANGE)?,
        memory_byte_len: View::u32_le_at(family, jt_family::MEMORY_BYTE_LEN)?,
        compression_level,
        vertex_version,
        vertex_bindings,
        vertex_quantization_bits,
        normal_quantization_factor,
        texture_quantization_bits,
        color_quantization_bits,
    })
}

struct ParsedJtPartitionNode {
    group_version: u16,
    child_object_ids: Vec<u32>,
    file_name: String,
    transformed_bounds: JtBounds,
    area: JtArea,
    vertex_count_range: [i32; 2],
    node_count_range: [i32; 2],
    polygon_count_range: [i32; 2],
    bounds: DisplayJtPartitionBounds,
}

fn parse_jt9_group_data(bytes: &[u8]) -> Option<(u16, Vec<u32>, &[u8])> {
    let mut view = View::over_retained(bytes);
    let version = view.u16_le()?;
    let count = view.u32_le()?;
    let children = view.read_counted(u64::from(count), 4, View::u32_le)?;
    Some((version, children, bytes.get(view.position()..)?))
}

fn parse_jt9_group_node_body(body: &[u8]) -> Option<(u16, Vec<u32>, &[u8])> {
    let (_, _, _, family) = parse_jt_base_node_body(body, 9)?;
    parse_jt9_group_data(family)
}

fn parse_jt9_partition_node_body(
    ctx: &DecodeContext<'_>,
    body: &[u8],
) -> Result<Option<ParsedJtPartitionNode>, CodecError> {
    let parsed = (|| {
        let (_, _, _, family) = parse_jt_base_node_body(body, 9)?;
        let (group_version, child_object_ids, family) = parse_jt9_group_data(family)?;
        let mut view = View::over_retained(family);
        let partition_flags = view.u32_le()?;
        if partition_flags & !1 != 0 {
            return None;
        }
        let name_count = usize::try_from(view.u32_le()?).ok()?;
        let name_bytes = view.take(name_count.checked_mul(2)?)?;
        let file_name = match ctx.utf16le_text(
            name_bytes,
            name_count,
            false,
            "retain DisplayJT partition name",
        ) {
            Ok(value) => value,
            Err(CodecError::Malformed(_)) => return None,
            Err(error) => return Some(Err(error)),
        };
        if let Err(error) = ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(file_name.len()),
            "validate DisplayJT partition name",
        ) {
            return Some(Err(error));
        }
        if file_name.is_empty() || file_name.chars().any(char::is_control) {
            return None;
        }
        let name_end = view.position();
        let f32_at = |offset: usize| FiniteBinary32::new(View::f32_le_at(family, offset)?);
        let bounds_at = |offset: usize| {
            let bounds = [
                [f32_at(offset)?, f32_at(offset + 4)?, f32_at(offset + 8)?],
                [
                    f32_at(offset + 12)?,
                    f32_at(offset + 16)?,
                    f32_at(offset + 20)?,
                ],
            ];
            JtBounds::from_finite(bounds)
        };
        let first_bounds = bounds_at(name_end)?;
        let mut cursor = name_end.checked_add(24)?;
        let transformed_bounds = if partition_flags & 1 == 0 {
            let transformed = bounds_at(cursor)?;
            cursor = cursor.checked_add(24)?;
            transformed
        } else {
            first_bounds
        };
        let area = JtArea::new(View::f32_le_at(family, cursor)?)?;
        cursor = cursor.checked_add(4)?;
        let count_range = |offset: usize| {
            let minimum = View::i32_le_at(family, offset)?;
            let maximum = View::i32_le_at(family, offset.checked_add(4)?)?;
            (minimum >= 0 && (maximum == -1 || maximum >= minimum)).then_some([minimum, maximum])
        };
        let vertex_count_range = count_range(cursor)?;
        let node_count_range = count_range(cursor + 8)?;
        let polygon_count_range = count_range(cursor + 16)?;
        cursor = cursor.checked_add(24)?;
        let bounds = if partition_flags & 1 != 0 {
            let bounds = bounds_at(cursor)?;
            cursor = cursor.checked_add(24)?;
            DisplayJtPartitionBounds::Untransformed(bounds)
        } else {
            DisplayJtPartitionBounds::Reserved(first_bounds)
        };
        (cursor == family.len()).then_some(Ok(ParsedJtPartitionNode {
            group_version,
            child_object_ids,
            file_name,
            transformed_bounds,
            area,
            vertex_count_range,
            node_count_range,
            polygon_count_range,
            bounds,
        }))
    })();
    parsed.transpose()
}

struct ParsedJtRangeLodNode {
    group_version: u16,
    child_object_ids: Vec<u32>,
    lod_version: u16,
    reserved_values: Vec<FiniteBinary32>,
    reserved_value: i32,
    range_version: u16,
    range_limits: JtRangeLimits,
    center: [FiniteBinary32; 3],
}

fn parse_jt_f32_vector(bytes: &[u8]) -> Option<(Vec<FiniteBinary32>, &[u8])> {
    let mut view = View::over_retained(bytes);
    let count = view.u32_le()?;
    let values = view.read_counted(u64::from(count), 4, View::f32_le)?;
    let values = values
        .into_iter()
        .map(FiniteBinary32::new)
        .collect::<Option<Vec<_>>>()?;
    Some((values, bytes.get(view.position()..)?))
}

fn parse_jt9_range_lod_node_body(body: &[u8]) -> Option<ParsedJtRangeLodNode> {
    let (_, _, _, family) = parse_jt_base_node_body(body, 9)?;
    let (group_version, child_object_ids, mut family) = parse_jt9_group_data(family)?;
    let lod_version = View::u16_le_at(family, 0)?;
    family = &family[2..];
    let (reserved_values, remaining) = parse_jt_f32_vector(family)?;
    family = remaining;
    let reserved_value = View::i32_le_at(family, 0)?;
    let range_version = View::u16_le_at(family, 4)?;
    let (range_limits, remaining) = parse_jt_f32_vector(&family[6..])?;
    let range_limits = JtRangeLimits::from_finite(range_limits).ok()?;
    let center = [
        FiniteBinary32::new(View::f32_le_at(remaining, 0)?)?,
        FiniteBinary32::new(View::f32_le_at(remaining, 4)?)?,
        FiniteBinary32::new(View::f32_le_at(remaining, 8)?)?,
    ];
    if remaining.len() != 12 {
        return None;
    }
    Some(ParsedJtRangeLodNode {
        group_version,
        child_object_ids,
        lod_version,
        reserved_values,
        reserved_value,
        range_version,
        range_limits,
        center,
    })
}

fn parse_jt9_geometric_transform_body(body: &[u8]) -> Option<(u8, u32, u16, JtTransformMatrix)> {
    let mut view = View::over_retained(body);
    let base_version = view.u16_le()?;
    let state_flags = view.u8()?;
    let field_inhibit_flags = view.u32_le()?;
    let version = view.u16_le()?;
    let stored_values_mask = view.u16_le()?;
    if base_version != 1 || version != 1 || state_flags & !0x0f != 0 || field_inhibit_flags != 0 {
        return None;
    }
    let mut matrix = [
        [
            FiniteBinary32::ONE,
            FiniteBinary32::ZERO,
            FiniteBinary32::ZERO,
            FiniteBinary32::ZERO,
        ],
        [
            FiniteBinary32::ZERO,
            FiniteBinary32::ONE,
            FiniteBinary32::ZERO,
            FiniteBinary32::ZERO,
        ],
        [
            FiniteBinary32::ZERO,
            FiniteBinary32::ZERO,
            FiniteBinary32::ONE,
            FiniteBinary32::ZERO,
        ],
        [
            FiniteBinary32::ZERO,
            FiniteBinary32::ZERO,
            FiniteBinary32::ZERO,
            FiniteBinary32::ONE,
        ],
    ];
    for index in 0..16 {
        if stored_values_mask & (0x8000 >> index) == 0 {
            continue;
        }
        let value = FiniteBinary32::new(view.f32_le()?)?;
        matrix[index / 4][index % 4] = value;
    }
    if !view.is_empty() {
        return None;
    }
    Some((
        state_flags,
        field_inhibit_flags,
        stored_values_mask,
        JtTransformMatrix::from_finite(matrix)?,
    ))
}

type ParsedJt9Material = (
    u8,
    u32,
    JtMaterialVersion,
    u16,
    [[UnitBinary32; 4]; 4],
    JtShininess,
);

fn parse_jt9_material_body(body: &[u8]) -> Option<ParsedJt9Material> {
    let base_version = View::u16_le_at(body, 0)?;
    let state_flags = *body.get(2)?;
    let field_inhibit_flags = View::u32_le_at(body, 3)?;
    let version = View::u16_le_at(body, 7)?;
    let data_flags = View::u16_le_at(body, 9)?;
    let expected_len = match version {
        1 => 79,
        2 => 83,
        _ => return None,
    };
    let source_blend_factor = (data_flags >> 6) & 0x1f;
    let destination_blend_factor = (data_flags >> 11) & 0x1f;
    if base_version != 1
        || state_flags & !0x0f != 0
        || field_inhibit_flags & !0x01ff != 0
        || data_flags & 0x000f != 0
        || source_blend_factor > 10
        || destination_blend_factor > 10
        || body.len() != expected_len
    {
        return None;
    }
    let scalar = |offset: usize| UnitBinary32::new(View::f32_le_at(body, offset)?);
    let rgba = |offset: usize| {
        Some([
            scalar(offset)?,
            scalar(offset + 4)?,
            scalar(offset + 8)?,
            scalar(offset + 12)?,
        ])
    };
    let colors = [rgba(11)?, rgba(27)?, rgba(43)?, rgba(59)?];
    let shininess = JtShininess::new(View::f32_le_at(body, 75)?)?;
    let reflectivity = if version == 2 {
        Some(UnitBinary32::try_from(View::f32_le_at(body, 79)?).ok()?)
    } else {
        None
    };
    let version = match (version, reflectivity) {
        (1, None) => JtMaterialVersion::One,
        (2, Some(value)) => JtMaterialVersion::Two(value),
        _ => return None,
    };
    Some((
        state_flags,
        field_inhibit_flags,
        version,
        data_flags,
        colors,
        shininess,
    ))
}

fn decimal_digits(mut value: usize) -> u64 {
    let mut digits = 1;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    digits
}

/// Decode the complete outer index of each `/Root/UG_PART/DisplayJT` stream.
pub(super) fn display_jt_indices(
    ctx: &DecodeContext<'_>,
    container: &Container,
) -> Result<Vec<DisplayJtIndex>, CodecError> {
    const JT_HEADER: &[u8] = b"Version ";
    let word_swapped_u64 = |bytes: &[u8]| -> Option<u64> {
        let high = View::u32_le_at(bytes, 0)?;
        let low = View::u32_le_at(bytes, 4)?;
        Some((u64::from(high) << 32) | u64::from(low))
    };
    let mut indices = Vec::new();
    for (index_ordinal, entry) in container
        .entries
        .iter()
        .filter(|entry| entry.name == "/Root/UG_PART/DisplayJT")
        .enumerate()
    {
        let parsed = (|| -> Result<Option<DisplayJtIndex>, CodecError> {
            let Some((source_offset, byte_len)) = entry.file_span() else {
                return Ok(None);
            };
            let Some(payload) = container.bounded_entry_bytes(source_offset, byte_len) else {
                return Ok(None);
            };
            let (Some(version), Some(declared_count)) =
                (View::u32_le_at(payload, 0), View::u32_le_at(payload, 4))
            else {
                return Ok(None);
            };
            let Ok(row_count) = usize::try_from(declared_count) else {
                return Ok(None);
            };
            let Some(table_end) = row_count
                .checked_mul(16)
                .and_then(|bytes| 8usize.checked_add(bytes))
            else {
                return Ok(None);
            };
            if table_end > payload.len() {
                return Ok(None);
            }
            if row_count == 0 {
                return Ok(None);
            }
            let row_size = u64::try_from(std::mem::size_of::<DisplayJtIndexRow>())
                .map_err(|_| ctx.refuse_codec_limit("retain DisplayJT index rows", 0, u64::MAX))?;
            ctx.charge_collection_items(u64::from(declared_count), "admit DisplayJT index rows")?;
            ctx.charge_retained(
                u64::from(declared_count)
                    .checked_mul(row_size)
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("retain DisplayJT index rows", 0, u64::MAX)
                    })?,
                "retain DisplayJT index rows",
            )?;
            let mut rows = Vec::new();
            cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
                &mut rows,
                row_count,
                "allocate DisplayJT index rows",
            )?;
            let mut previous_header_offset = None;
            for ordinal in 0..row_count {
                let row_offset = 8 + ordinal * 16;
                let Some(value) = payload
                    .get(row_offset..row_offset + 8)
                    .and_then(word_swapped_u64)
                    .and_then(NonZeroU64::new)
                else {
                    return Ok(None);
                };
                let Some(header_offset) = payload
                    .get(row_offset + 8..row_offset + 16)
                    .and_then(word_swapped_u64)
                else {
                    return Ok(None);
                };
                if header_offset > u64::from(u32::MAX) {
                    return Ok(None);
                }
                let Ok(header_offset_usize) = usize::try_from(header_offset) else {
                    return Ok(None);
                };
                if header_offset_usize < table_end
                    || !payload
                        .get(header_offset_usize..)
                        .is_some_and(|tail| tail.starts_with(JT_HEADER))
                    || previous_header_offset.is_some_and(|previous| header_offset <= previous)
                {
                    return Ok(None);
                }
                previous_header_offset = Some(header_offset);
                ctx.charge_entities(1, "admit DisplayJT index row")?;
                let id_len = cadmpeg_core::decode::u64_from_index("nx:display-jt:index#".len())
                    .checked_add(decimal_digits(index_ordinal))
                    .and_then(|len| {
                        len.checked_add(cadmpeg_core::decode::u64_from_index("-row-".len()))
                    })
                    .and_then(|len| len.checked_add(decimal_digits(ordinal)))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("retain DisplayJT index row identity", 0, u64::MAX)
                    })?;
                ctx.charge_retained(id_len, "retain DisplayJT index row identity")?;
                rows.push(DisplayJtIndexRow {
                    id: format!("nx:display-jt:index#{index_ordinal}-row-{ordinal}"),
                    ordinal: u32::try_from(ordinal).map_err(|_| {
                        ctx.refuse_codec_limit(
                            "DisplayJT count exceeds u32",
                            u64::from(u32::MAX),
                            cadmpeg_core::decode::u64_from_index(ordinal),
                        )
                    })?,
                    header_offset: u32::try_from(header_offset).map_err(|_| {
                        CodecError::malformed("DisplayJT header offset exceeds u32")
                    })?,
                    value,
                    source_offset: source_offset + cadmpeg_core::decode::u64_from_index(row_offset),
                });
            }
            ctx.charge_entities(1, "admit DisplayJT index entity")?;
            let id_len = cadmpeg_core::decode::u64_from_index("nx:display-jt:index#".len())
                .checked_add(decimal_digits(index_ordinal))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("retain DisplayJT index identity", 0, u64::MAX)
                })?;
            ctx.charge_retained(id_len, "retain DisplayJT index identity")?;
            Ok(DisplayJtIndex::new(
                format!("nx:display-jt:index#{index_ordinal}"),
                version,
                rows,
                source_offset,
            )
            .ok())
        })()?;
        if let Some(index) = parsed {
            ctx.reserve_retained_vec(&mut indices, 1, "admit DisplayJT index")?;
            indices.push(index);
        }
    }
    Ok(indices)
}

/// Decode complete standard JT headers and tables of contents from an outer index.
pub(super) fn display_jt_documents(
    ctx: &DecodeContext<'_>,
    container: &Container,
    indices: &[DisplayJtIndex],
) -> Result<Vec<DisplayJtDocument>, CodecError> {
    let mut entries = container
        .entries
        .iter()
        .filter(|entry| entry.name == "/Root/UG_PART/DisplayJT");
    let Some(entry) = entries.next() else {
        return Ok(Vec::new());
    };
    if entries.next().is_some() {
        return Ok(Vec::new());
    }
    let Some((stream_source_offset, stream_byte_len)) = entry.file_span() else {
        return Ok(Vec::new());
    };
    let Some(stream) = container.bounded_entry_bytes(stream_source_offset, stream_byte_len) else {
        return Ok(Vec::new());
    };
    let [index] = indices else {
        return Ok(Vec::new());
    };
    let mut documents = Vec::new();
    let mut rows = index.rows.iter().peekable();
    while let Some(row) = rows.next() {
        ctx.charge_work(1, "scan DisplayJT document")?;
        let Ok(document_start) = usize::try_from(row.header_offset) else {
            return Ok(Vec::new());
        };
        let document_end = rows.peek().map_or(stream.len(), |next| {
            cadmpeg_core::decode::index_from_u32(next.header_offset)
        });
        let Some(document) = stream.get(document_start..document_end) else {
            return Ok(Vec::new());
        };
        let Some(version_bytes) = document.get(..jt_hdr::BYTE_ORDER) else {
            return Ok(Vec::new());
        };
        let Some(version_field) = std::str::from_utf8(version_bytes).ok() else {
            return Ok(Vec::new());
        };
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(version_field.len()),
            "retain DisplayJT version text",
        )?;
        let Ok(version) = JtVersionField::new(version_field.to_owned()) else {
            return Ok(Vec::new());
        };
        let Some(&byte_order) = document.get(jt_hdr::BYTE_ORDER) else {
            return Ok(Vec::new());
        };
        if byte_order != 0 || document.get(jt_hdr::RESERVED..jt_hdr::TOC_OFFSET) != Some(&[0; 4]) {
            return Ok(Vec::new());
        }
        let Some(toc_offset) = View::u32_le_at(document, jt_hdr::TOC_OFFSET) else {
            return Ok(Vec::new());
        };
        let Some(lsg_segment_id) = document
            .get(jt_hdr::LSG_SEGMENT_ID..jt_hdr::LEN)
            .and_then(|bytes| <[u8; 16]>::try_from(bytes).ok())
        else {
            return Ok(Vec::new());
        };
        let Ok(toc_start) = usize::try_from(toc_offset) else {
            return Ok(Vec::new());
        };
        let Some(toc_count) = View::u32_le_at(document, toc_start) else {
            return Ok(Vec::new());
        };
        let Ok(toc_count_usize) = usize::try_from(toc_count) else {
            return Ok(Vec::new());
        };
        if toc_count_usize == 0 {
            return Ok(Vec::new());
        }
        let Some(toc_end) = toc_start
            .checked_add(4)
            .and_then(|start| start.checked_add(toc_count_usize.checked_mul(jt_toc::LEN)?))
        else {
            return Ok(Vec::new());
        };
        if toc_end > document.len() {
            return Ok(Vec::new());
        }
        let document_key = row
            .id
            .rsplit_once('#')
            .map_or(row.id.as_str(), |(_, key)| key);
        ctx.charge_work(u64::from(toc_count), "scan DisplayJT table of contents")?;
        let mut toc_entries = ctx.retained_vec(toc_count_usize, "admit DisplayJT toc entries")?;
        for ordinal in 0..toc_count_usize {
            let offset = toc_start + 4 + ordinal * jt_toc::LEN;
            let Some(bytes) = View::over_retained(&document[offset..offset + jt_toc::LEN])
                .array::<{ jt_toc::LEN }>()
            else {
                return Ok(Vec::new());
            };
            let [segment_id @ .., o0, o1, o2, o3, l0, l1, l2, l3, a0, a1, a2, a3] = bytes;
            let segment_offset = assemble_u32_le([o0, o1, o2, o3]);
            let segment_byte_len = assemble_u32_le([l0, l1, l2, l3]);
            let attributes = [a0, a1, a2, a3];
            let Some(segment_end) = usize::try_from(segment_offset).ok().and_then(|start| {
                start.checked_add(cadmpeg_core::decode::index_from_u32(segment_byte_len))
            }) else {
                return Ok(Vec::new());
            };
            if segment_byte_len == 0
                || (cadmpeg_core::decode::index_from_u32(segment_offset)) < toc_end
                || segment_end > document.len()
            {
                return Ok(Vec::new());
            }
            ctx.charge_entities(1, "admit DisplayJT toc entry")?;
            let id_len = cadmpeg_core::decode::u64_from_index("nx:display-jt:toc-entry#".len())
                .checked_add(cadmpeg_core::decode::u64_from_index(document_key.len()))
                .and_then(|len| len.checked_add(1))
                .and_then(|len| len.checked_add(decimal_digits(ordinal)))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("retain DisplayJT toc identity", 0, u64::MAX)
                })?;
            ctx.charge_retained(id_len, "retain DisplayJT toc identity")?;
            toc_entries.push(DisplayJtTocEntry {
                id: format!("nx:display-jt:toc-entry#{document_key}-{ordinal}"),
                ordinal: u32::try_from(ordinal).map_err(|_| {
                    ctx.refuse_codec_limit(
                        "DisplayJT count exceeds u32",
                        u64::from(u32::MAX),
                        cadmpeg_core::decode::u64_from_index(ordinal),
                    )
                })?,
                segment_id,
                segment_offset,
                segment_byte_len,
                attributes,
                source_offset: stream_source_offset
                    + cadmpeg_core::decode::u64_from_index(document_start)
                    + cadmpeg_core::decode::u64_from_index(offset),
            });
        }
        ctx.charge_entities(1, "admit DisplayJT document entity")?;
        let id_len = cadmpeg_core::decode::u64_from_index("nx:display-jt:document#".len())
            .checked_add(cadmpeg_core::decode::u64_from_index(document_key.len()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit("retain DisplayJT document identity", 0, u64::MAX)
            })?;
        ctx.charge_retained(id_len, "retain DisplayJT document identity")?;
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(row.id.len()),
            "retain DisplayJT document index reference",
        )?;
        ctx.reserve_retained_vec(&mut documents, 1, "admit DisplayJT document")?;
        documents.push(DisplayJtDocument {
            id: format!("nx:display-jt:document#{document_key}"),
            index_row: row.id.clone(),
            version,
            toc_offset,
            lsg_segment_id,
            toc_entries,
            physical_byte_len: cadmpeg_core::decode::u64_from_index(document.len()),
            source_offset: stream_source_offset
                + cadmpeg_core::decode::u64_from_index(document_start),
        });
    }
    Ok(documents)
}

/// Decode every segment declared by complete embedded JT documents.
pub(super) fn display_jt_segments(
    budget: (&DecodeContext<'_>, View<'_>),
    container: &Container,
    documents: &[DisplayJtDocument],
) -> Result<Vec<DisplayJtSegment>, CodecError> {
    let mut segments = Vec::new();
    for document in documents {
        let document_key = document
            .id
            .split_once('#')
            .map_or(document.id.as_str(), |(_, key)| key);
        let Some(bytes) =
            container.bounded_entry_bytes(document.source_offset, document.physical_byte_len)
        else {
            return Ok(Vec::new());
        };
        for entry in &document.toc_entries {
            let (Ok(segment_start), Ok(segment_len)) = (
                usize::try_from(entry.segment_offset),
                usize::try_from(entry.segment_byte_len),
            ) else {
                return Ok(Vec::new());
            };
            let Some(segment_end) = segment_start.checked_add(segment_len) else {
                return Ok(Vec::new());
            };
            let Some(segment) = bytes.get(segment_start..segment_end) else {
                return Ok(Vec::new());
            };
            let Some(segment_id) = segment
                .get(..16)
                .and_then(|bytes| <[u8; 16]>::try_from(bytes).ok())
            else {
                return Ok(Vec::new());
            };
            let Some(segment_type) = View::u32_le_at(segment, 16) else {
                return Ok(Vec::new());
            };
            let Some(header_byte_len) = View::u32_le_at(segment, 20) else {
                return Ok(Vec::new());
            };
            let Some(attribute_type) = View::u32_be_at(&entry.attributes, 0) else {
                return Ok(Vec::new());
            };
            if segment_id != entry.segment_id
                || segment_type != attribute_type
                || header_byte_len != entry.segment_byte_len
            {
                return Ok(Vec::new());
            }
            let payload = &segment[24..];
            let compression = if payload.get(..4) == Some(2_u32.to_le_bytes().as_slice()) {
                let Some(compressed_data_byte_len) = View::u32_le_at(payload, 4) else {
                    return Ok(Vec::new());
                };
                let Some(&algorithm) = payload.get(8) else {
                    return Ok(Vec::new());
                };
                let compressed = &payload[9..];
                let Ok(compressed_byte_len) = u32::try_from(compressed.len()) else {
                    return Ok(Vec::new());
                };
                let Ok(envelope) = JtCompressionEnvelope::try_new(
                    2,
                    compressed_data_byte_len,
                    algorithm,
                    compressed_byte_len,
                ) else {
                    return Ok(Vec::new());
                };
                let Some(member) = document
                    .source_offset
                    .checked_add(u64::from(entry.segment_offset))
                    .and_then(|offset| offset.checked_add(33))
                    .and_then(cadmpeg_core::decode::index_from_u64)
                    .and_then(|start| {
                        start
                            .checked_add(compressed.len())
                            .and_then(|end| budget.1.child(start, end))
                    })
                else {
                    return Ok(Vec::new());
                };
                let Some(inflated) = inflate_display_jt(budget.0, member)? else {
                    return Ok(Vec::new());
                };
                Some(DisplayJtCompression {
                    envelope,
                    inflated_sha256: Sha256Digest::digest_for_decode(
                        budget.0,
                        &inflated,
                        "retain DisplayJT hash",
                    )?,
                })
            } else {
                None
            };
            let ctx = budget.0;
            let digits = entry
                .ordinal
                .checked_ilog10()
                .map_or(1, |value| u64::from(value) + 1);
            let text_bytes = display_jt_text_size(
                ctx,
                &[
                    "nx:display-jt:segment#",
                    document_key,
                    "-",
                    &document.id,
                    &entry.id,
                ],
                digits,
            )?;
            ctx.reserve_record_vec(&mut segments, 1, text_bytes, "store DisplayJT segment")?;
            segments.push(DisplayJtSegment {
                id: format!("nx:display-jt:segment#{document_key}-{}", entry.ordinal),
                document: document.id.clone(),
                toc_entry: entry.id.clone(),
                segment_id,
                segment_type,
                segment_byte_len: header_byte_len,
                payload_sha256: Sha256Digest::digest_for_decode(
                    ctx,
                    payload,
                    "retain DisplayJT hash",
                )?,
                compression,
                source_offset: document.source_offset + u64::from(entry.segment_offset),
            });
        }
    }
    Ok(segments)
}

/// Decode complete object-element sequences from type-7 shape-LOD segments.
pub(super) fn display_jt_shape_lod_elements(
    budget: (&DecodeContext<'_>, View<'_>),
    container: &Container,
    segments: &[DisplayJtSegment],
) -> Result<Vec<DisplayJtShapeLodElement>, CodecError> {
    const SEGMENT_TAIL: [u8; 6] = [1, 0, 0, 0, 0, 0];
    let mut elements = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 7) {
        let Some(bytes) = container
            .bounded_entry_bytes(segment.source_offset, u64::from(segment.segment_byte_len))
        else {
            return Ok(Vec::new());
        };
        let payload = &bytes[24..];
        let Some((parsed, framed_end)) = parse_jt_element_sequence(budget.0, payload)? else {
            return Ok(Vec::new());
        };
        if payload.get(framed_end..) != Some(SEGMENT_TAIL.as_slice()) {
            return Ok(Vec::new());
        }
        for (ordinal, element) in parsed.into_iter().enumerate() {
            if element.object_base_type != 4 {
                return Ok(Vec::new());
            }
            let ctx = budget.0;
            let text_bytes = display_jt_text_size(
                ctx,
                &[&segment.id, "-element-", &segment.id],
                decimal_digits(ordinal),
            )?;
            ctx.reserve_record_vec(
                &mut elements,
                1,
                text_bytes,
                "store DisplayJT shape element",
            )?;
            elements.push(DisplayJtShapeLodElement {
                id: format!("{}-element-{ordinal}", segment.id),
                segment: segment.id.clone(),
                ordinal: u32::try_from(ordinal).map_err(|_| {
                    ctx.refuse_codec_limit(
                        "DisplayJT count exceeds u32",
                        u64::from(u32::MAX),
                        cadmpeg_core::decode::u64_from_index(ordinal),
                    )
                })?,
                object_type_id: element.object_type_id,
                object_id: element.object_id,
                body_byte_len: u32::try_from(element.body.len()).map_err(|_| {
                    ctx.refuse_codec_limit(
                        "DisplayJT count exceeds u32",
                        u64::from(u32::MAX),
                        cadmpeg_core::decode::u64_from_index(element.body.len()),
                    )
                })?,
                body_sha256: Sha256Digest::digest_for_decode(
                    ctx,
                    element.body,
                    "retain DisplayJT hash",
                )?,
                source_offset: segment.source_offset
                    + 24
                    + cadmpeg_core::decode::u64_from_index(element.offset),
            });
        }
    }
    Ok(elements)
}

/// Decode fixed headers from JT 9 tri-strip shape-LOD elements.
pub(super) fn display_jt_tri_strip_lod_headers(
    ctx: &DecodeContext<'_>,
    container: &Container,
    elements: &[DisplayJtShapeLodElement],
) -> Result<Vec<DisplayJtTriStripLodHeader>, CodecError> {
    const TRI_STRIP_LOD_TYPE: [u8; 16] = [
        0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut headers = Vec::new();
    for element in elements
        .iter()
        .filter(|element| element.object_type_id == TRI_STRIP_LOD_TYPE)
    {
        let Some(body_start) = element.source_offset.checked_add(25) else {
            return Ok(Vec::new());
        };
        let Some(body) =
            container.bounded_entry_bytes(body_start, u64::from(element.body_byte_len))
        else {
            return Ok(Vec::new());
        };
        let Some((
            vertex_bindings,
            topological_mesh_version,
            vertex_records_object_id,
            compressed_lod_version,
            compressed_representation,
        )) = parse_jt9_tri_strip_lod_header(body)
        else {
            return Ok(Vec::new());
        };
        ctx.reserve_record_vec(&mut headers, 1, 0, "nx JT tri-strip headers")?;
        headers.push(DisplayJtTriStripLodHeader {
            id: ctx.join_retained(
                &[&element.id, "-tri-strip-header"],
                "",
                "nx JT tri-strip identity",
            )?,
            element: ctx.join_retained(&[&element.id], "", "nx JT tri-strip element reference")?,
            base_version: 1,
            vertex_version: 1,
            vertex_bindings,
            topological_mesh_version,
            vertex_records_object_id,
            compressed_lod_version,
            compressed_representation_byte_len: u32::try_from(compressed_representation.len())
                .map_err(|_| {
                    ctx.refuse_codec_limit(
                        "DisplayJT count exceeds u32",
                        u64::from(u32::MAX),
                        cadmpeg_core::decode::u64_from_index(compressed_representation.len()),
                    )
                })?,
            compressed_representation_sha256: Sha256Digest::digest_for_decode(
                ctx,
                compressed_representation,
                "retain DisplayJT hash",
            )?,
            source_offset: element.source_offset + 25,
        });
    }
    Ok(headers)
}

/// Decode the initial face-degree packet from each JT 9 topological mesh.
pub(super) fn display_jt_initial_face_degree_symbols(
    ctx: &DecodeContext<'_>,
    container: &Container,
    elements: &[DisplayJtShapeLodElement],
) -> Result<Vec<DisplayJtInitialFaceDegreeSymbols>, CodecError> {
    const TRI_STRIP_LOD_TYPE: [u8; 16] = [
        0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut vectors = Vec::new();
    for element in elements
        .iter()
        .filter(|element| element.object_type_id == TRI_STRIP_LOD_TYPE)
    {
        let Some(body_start) = element.source_offset.checked_add(25) else {
            return Ok(Vec::new());
        };
        let Some(body) =
            container.bounded_entry_bytes(body_start, u64::from(element.body_byte_len))
        else {
            return Ok(Vec::new());
        };
        let Some((_, _, _, _, representation)) = parse_jt9_tri_strip_lod_header(body) else {
            return Ok(Vec::new());
        };
        let Some((residuals, packet_byte_len)) =
            crate::jt::decode_int32_cdp2(ctx, representation, 0)?
        else {
            return Ok(Vec::new());
        };
        let degrees =
            crate::jt::unpack_predictor_residuals(ctx, &residuals, crate::jt::Predictor::Null)?;
        let Some(packet) = representation.get(..packet_byte_len) else {
            return Ok(Vec::new());
        };
        ctx.reserve_record_vec(&mut vectors, 1, 0, "nx JT face degree records")?;
        vectors.push(DisplayJtInitialFaceDegreeSymbols {
            id: ctx.join_retained(
                &[&element.id, "-initial-face-degrees"],
                "",
                "nx JT face degree identity",
            )?,
            element: ctx.join_retained(&[&element.id], "", "nx JT face degree reference")?,
            degrees,
            packet_byte_len: u32::try_from(packet_byte_len).map_err(|_| {
                ctx.refuse_codec_limit(
                    "DisplayJT count exceeds u32",
                    u64::from(u32::MAX),
                    cadmpeg_core::decode::u64_from_index(packet_byte_len),
                )
            })?,
            packet_sha256: Sha256Digest::digest_for_decode(ctx, packet, "retain DisplayJT hash")?,
            source_offset: element.source_offset + 45,
        });
    }
    Ok(vectors)
}

/// Bound every JT 9 topology vector and decode the following vertex-record header.
#[derive(Default)]
pub(super) struct DisplayJtTopologyArrays {
    pub(super) sequences: Vec<DisplayJtTopologyPacketSequence>,
    pub(super) vertex_headers: Vec<DisplayJtCompressedVertexRecordsHeader>,
    pub(super) coordinate_headers: Vec<DisplayJtVertexCoordinateArrayHeader>,
}

pub(super) fn display_jt_topology_packet_sequences(
    ctx: &DecodeContext<'_>,
    container: &Container,
    elements: &[DisplayJtShapeLodElement],
) -> Result<DisplayJtTopologyArrays, CodecError> {
    const TRI_STRIP_LOD_TYPE: [u8; 16] = [
        0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut sequences = Vec::new();
    let mut headers = Vec::new();
    let mut coordinate_headers = Vec::new();
    for element in elements
        .iter()
        .filter(|element| element.object_type_id == TRI_STRIP_LOD_TYPE)
    {
        let Some(body_start) = element.source_offset.checked_add(25) else {
            return Ok(DisplayJtTopologyArrays::default());
        };
        let Some(body) =
            container.bounded_entry_bytes(body_start, u64::from(element.body_byte_len))
        else {
            return Ok(DisplayJtTopologyArrays::default());
        };
        let Some((lod_vertex_bindings, _, _, _, representation)) =
            parse_jt9_tri_strip_lod_header(body)
        else {
            return Ok(DisplayJtTopologyArrays::default());
        };
        let mut cursor = 0usize;
        let Some(high_degree_lane_count) =
            jt9_topology_high_degree_lane_count(ctx, representation, lod_vertex_bindings)?
        else {
            return Ok(DisplayJtTopologyArrays::default());
        };
        let Some(role_count) = 23usize.checked_add(high_degree_lane_count) else {
            return Ok(DisplayJtTopologyArrays::default());
        };
        let roles = TopologyContext::ALL
            .map(TopologyPacketRole::FaceDegrees)
            .into_iter()
            .chain([
                TopologyPacketRole::VertexValences,
                TopologyPacketRole::VertexGroups,
                TopologyPacketRole::VertexFlags,
            ])
            .chain(TopologyContext::ALL.map(TopologyPacketRole::FaceAttributeMasks))
            .chain([
                TopologyPacketRole::FaceAttributeMasks7Next30,
                TopologyPacketRole::FaceAttributeMasks7Upper4,
            ])
            .chain(
                (0..high_degree_lane_count).map(TopologyPacketRole::HighDegreeFaceAttributeMasks),
            )
            .chain([
                TopologyPacketRole::SplitFaceSymbols,
                TopologyPacketRole::SplitFacePositions,
            ]);
        let mut packets = ctx.retained_vec(role_count, "nx JT topology packets")?;
        for role in roles {
            let Some(remaining) = representation.get(cursor..) else {
                return Ok(DisplayJtTopologyArrays::default());
            };
            let Some((value_count, codec, byte_len)) =
                crate::jt::frame_int32_cdp2(ctx, remaining, 0)?
            else {
                return Ok(DisplayJtTopologyArrays::default());
            };
            let Some(packet_end) = cursor.checked_add(byte_len) else {
                return Ok(DisplayJtTopologyArrays::default());
            };
            let Some(packet) = representation.get(cursor..packet_end) else {
                return Ok(DisplayJtTopologyArrays::default());
            };
            let (Ok(byte_len), Ok(representation_offset)) =
                (u32::try_from(byte_len), u32::try_from(cursor))
            else {
                return Ok(DisplayJtTopologyArrays::default());
            };
            let values = match crate::jt::decode_int32_cdp2(ctx, packet, 0)? {
                Some((residuals, decoded_byte_len)) if decoded_byte_len == packet.len() => {
                    let predictor = match role {
                        TopologyPacketRole::VertexFlags | TopologyPacketRole::SplitFaceSymbols => {
                            crate::jt::Predictor::Lag1
                        }
                        _ => crate::jt::Predictor::Null,
                    };
                    Some(crate::jt::unpack_predictor_residuals(
                        ctx, &residuals, predictor,
                    )?)
                }
                _ => None,
            };
            packets.push(DisplayJtTopologyPacket {
                role,
                value_count,
                codec,
                byte_len,
                sha256: Sha256Digest::digest_for_decode(ctx, packet, "retain DisplayJT hash")?,
                representation_offset,
                values,
            });
            cursor += cadmpeg_core::decode::index_from_u32(byte_len);
        }
        let Some(composite_hash) = View::u32_le_at(representation, cursor) else {
            return Ok(DisplayJtTopologyArrays::default());
        };
        cursor += 4;
        let Some(required_header_end) = cursor.checked_add(16) else {
            return Ok(DisplayJtTopologyArrays::default());
        };
        let Some(required_header) = representation
            .get(cursor..required_header_end)
            .and_then(|bytes| View::over_retained(bytes).array::<16>())
        else {
            return Ok(DisplayJtTopologyArrays::default());
        };
        let [bindings @ .., q0, q1, q2, q3, n0, n1, n2, n3] = required_header;
        let vertex_bindings = assemble_u64_le(bindings);
        let quantization = [q0, q1, q2, q3];
        if quantization[0] > 24
            || quantization[1] > 13
            || quantization[2] > 24
            || quantization[3] > 24
        {
            return Ok(DisplayJtTopologyArrays::default());
        }
        if vertex_bindings != lod_vertex_bindings {
            return Ok(DisplayJtTopologyArrays::default());
        }
        let topological_vertex_count = assemble_u32_le([n0, n1, n2, n3]);
        let (vertex_attribute_count, vertex_header_byte_len) = if topological_vertex_count == 0 {
            (0, 16)
        } else {
            let Some(attribute_end) = cursor.checked_add(20) else {
                return Ok(DisplayJtTopologyArrays::default());
            };
            let Some(attribute_bytes) = representation
                .get(cursor + 16..attribute_end)
                .and_then(|bytes| View::over_retained(bytes).array::<4>())
            else {
                return Ok(DisplayJtTopologyArrays::default());
            };
            (assemble_u32_le(attribute_bytes), 20)
        };
        if i32::try_from(topological_vertex_count).is_err()
            || i32::try_from(vertex_attribute_count).is_err()
        {
            return Ok(DisplayJtTopologyArrays::default());
        }
        let arrays = &representation[cursor + vertex_header_byte_len..];
        let (Ok(topology_byte_len), Ok(compressed_arrays_byte_len)) =
            (u32::try_from(cursor), u32::try_from(arrays.len()))
        else {
            return Ok(DisplayJtTopologyArrays::default());
        };
        let representation_source_offset = element.source_offset + 45;
        if topological_vertex_count != 0 {
            let Some(coordinate_header) = View::over_retained(arrays).array::<32>() else {
                return Ok(DisplayJtTopologyArrays::default());
            };
            let [n0, n1, n2, n3, component_count, ranges @ ..] = coordinate_header;
            let unique_vertex_count = assemble_u32_le([n0, n1, n2, n3]);
            if unique_vertex_count != topological_vertex_count || component_count != 3 {
                return Ok(DisplayJtTopologyArrays::default());
            }
            let mut component_ranges = [QuantizedRange::ZERO; 3];
            let mut component_quantization_bits = [0; 3];
            for (component, &[m0, m1, m2, m3, x0, x1, x2, x3, bits]) in
                ranges.as_chunks::<9>().0.iter().enumerate()
            {
                let minimum = assemble_f32_le([m0, m1, m2, m3]);
                let maximum = assemble_f32_le([x0, x1, x2, x3]);
                let Some(range) = QuantizedRange::new(minimum, maximum) else {
                    return Ok(DisplayJtTopologyArrays::default());
                };
                if bits > 32 || bits != quantization[0] {
                    return Ok(DisplayJtTopologyArrays::default());
                }
                component_ranges[component] = range;
                component_quantization_bits[component] = bits;
            }
            let compressed_components = &arrays[32..];
            let Ok(compressed_components_byte_len) = u32::try_from(compressed_components.len())
            else {
                return Ok(DisplayJtTopologyArrays::default());
            };
            let Ok(vertex_header_byte_len_u64) = u64::try_from(vertex_header_byte_len) else {
                return Ok(DisplayJtTopologyArrays::default());
            };
            ctx.reserve_record_vec(&mut coordinate_headers, 1, 0, "nx JT coordinate headers")?;
            coordinate_headers.push(DisplayJtVertexCoordinateArrayHeader {
                id: ctx.join_retained(
                    &[&element.id, "-coordinate-array-header"],
                    "",
                    "nx JT coordinate header identity",
                )?,
                element: ctx.join_retained(&[&element.id], "", "nx JT element reference")?,
                unique_vertex_count,
                component_count,
                component_ranges,
                component_quantization_bits,
                compressed_components_byte_len,
                compressed_components_sha256: Sha256Digest::digest_for_decode(
                    ctx,
                    compressed_components,
                    "retain DisplayJT hash",
                )?,
                source_offset: representation_source_offset
                    + u64::from(topology_byte_len)
                    + vertex_header_byte_len_u64,
            });
        }
        ctx.reserve_record_vec(&mut sequences, 1, 0, "nx JT topology sequences")?;
        sequences.push(DisplayJtTopologyPacketSequence {
            id: ctx.join_retained(
                &[&element.id, "-topology-packets"],
                "",
                "nx JT topology sequence identity",
            )?,
            element: ctx.join_retained(&[&element.id], "", "nx JT element reference")?,
            packets,
            composite_hash,
            topology_byte_len,
            source_offset: representation_source_offset,
        });
        ctx.reserve_record_vec(&mut headers, 1, 0, "nx JT vertex headers")?;
        headers.push(DisplayJtCompressedVertexRecordsHeader {
            id: ctx.join_retained(
                &[&element.id, "-vertex-records-header"],
                "",
                "nx JT vertex header identity",
            )?,
            element: ctx.join_retained(&[&element.id], "", "nx JT element reference")?,
            vertex_bindings,
            vertex_quantization_bits: quantization[0],
            normal_quantization_factor: quantization[1],
            texture_quantization_bits: quantization[2],
            color_quantization_bits: quantization[3],
            topological_vertex_count,
            vertex_attribute_count,
            compressed_arrays_byte_len,
            compressed_arrays_sha256: Sha256Digest::digest_for_decode(
                ctx,
                arrays,
                "retain DisplayJT hash",
            )?,
            source_offset: representation_source_offset + u64::from(topology_byte_len),
        });
    }
    Ok(DisplayJtTopologyArrays {
        sequences,
        vertex_headers: headers,
        coordinate_headers,
    })
}

/// Decode every complete JT 9 coordinate array.
pub(super) fn display_jt_vertex_coordinates(
    ctx: &DecodeContext<'_>,
    container: &Container,
    headers: &[DisplayJtVertexCoordinateArrayHeader],
) -> Result<Vec<DisplayJtVertexCoordinates>, CodecError> {
    let mut arrays = Vec::new();
    for header in headers {
        let Some(start) = header.source_offset.checked_add(32) else {
            return Ok(Vec::new());
        };
        let Some(bytes) =
            container.bounded_entry_bytes(start, u64::from(header.compressed_components_byte_len))
        else {
            return Ok(Vec::new());
        };
        let Some(crate::jt::DecodedVertexArray {
            values: points_m,
            hash: coordinate_hash,
            byte_len: consumed,
        }) = crate::jt::decode_vertex_coordinates(
            ctx,
            bytes,
            cadmpeg_core::decode::index_from_u32(header.unique_vertex_count),
            header.component_ranges,
            header.component_quantization_bits,
        )?
        else {
            return Ok(Vec::new());
        };
        let Ok(consumed) = u32::try_from(consumed) else {
            return Ok(Vec::new());
        };
        ctx.reserve_record_vec(&mut arrays, 1, 0, "nx JT vertex coordinates")?;
        arrays.push(DisplayJtVertexCoordinates {
            id: replace_jt_text_once(
                ctx,
                &header.id,
                "coordinate-array-header",
                "vertex-coordinates",
                "nx JT coordinate identity",
            )?,
            header: ctx.join_retained(&[&header.id], "", "nx JT coordinate header reference")?,
            points_m,
            coordinate_hash,
            byte_len: consumed,
            source_offset: header.source_offset + 32,
        });
    }
    Ok(arrays)
}

/// Reconstruct every complete JT 9 polygon mesh from its dual-mesh lanes.
pub(super) fn display_jt_polygon_meshes(
    ctx: &DecodeContext<'_>,
    sequences: &[DisplayJtTopologyPacketSequence],
    coordinate_headers: &[DisplayJtVertexCoordinateArrayHeader],
) -> Result<Vec<DisplayJtPolygonMesh>, CodecError> {
    let mut meshes = Vec::new();
    for sequence in sequences {
        let values = |role: TopologyPacketRole| {
            sequence
                .packets
                .iter()
                .find(|packet| packet.role == role)?
                .values
                .as_deref()
        };
        let Some(valences) = values(TopologyPacketRole::VertexValences) else {
            return Ok(Vec::new());
        };
        if valences.is_empty() {
            continue;
        }
        let Some(coordinate_header) = coordinate_headers
            .iter()
            .find(|header| header.element == sequence.element)
        else {
            return Ok(Vec::new());
        };
        let mut degrees = Vec::new();
        let _degrees_reservation =
            ctx.reserve_temporary_vec(&mut degrees, 8, "nx JT face degree lanes")?;
        for context in TopologyContext::ALL {
            let Some(lane) = values(TopologyPacketRole::FaceDegrees(context)) else {
                return Ok(Vec::new());
            };
            degrees.push(lane);
        }
        let mut attribute_masks = Vec::new();
        let _attribute_reservation =
            ctx.reserve_temporary_vec(&mut attribute_masks, 8, "nx JT attribute mask lanes")?;
        for context in TopologyContext::ALL {
            let Some(lane) = values(TopologyPacketRole::FaceAttributeMasks(context)) else {
                return Ok(Vec::new());
            };
            attribute_masks.push(lane);
        }
        let Some(context_7_next_30) = values(TopologyPacketRole::FaceAttributeMasks7Next30) else {
            return Ok(Vec::new());
        };
        let Some(context_7_upper_4) = values(TopologyPacketRole::FaceAttributeMasks7Upper4) else {
            return Ok(Vec::new());
        };
        let large_lane_count = sequence
            .packets
            .iter()
            .filter(|packet| {
                matches!(
                    packet.role,
                    TopologyPacketRole::HighDegreeFaceAttributeMasks(_)
                )
            })
            .count();
        let mut large_lanes = Vec::new();
        let _large_lane_reservation = ctx.reserve_temporary_vec(
            &mut large_lanes,
            large_lane_count,
            "nx JT large mask lanes",
        )?;
        for packet in sequence.packets.iter().filter(|packet| {
            matches!(
                packet.role,
                TopologyPacketRole::HighDegreeFaceAttributeMasks(_)
            )
        }) {
            let Some(lane) = packet.values.as_deref() else {
                return Ok(Vec::new());
            };
            large_lanes.push(lane);
        }
        let Some(large_word_count) = large_lanes
            .iter()
            .try_fold(0usize, |sum, lane| sum.checked_add(lane.len()))
        else {
            return Ok(Vec::new());
        };
        let mut large_words = Vec::new();
        let _large_word_reservation = ctx.reserve_temporary_vec(
            &mut large_words,
            large_word_count,
            "nx JT large mask words",
        )?;
        large_words.extend(large_lanes.into_iter().flatten().copied());
        let (Ok(degrees), Ok(attribute_masks)) = (
            <[_; 8]>::try_from(degrees),
            <[_; 8]>::try_from(attribute_masks),
        ) else {
            return Ok(Vec::new());
        };
        let Some(polygons) = crate::jt_topology::decode(
            ctx,
            degrees,
            valences,
            values(TopologyPacketRole::VertexGroups).unwrap_or_default(),
            values(TopologyPacketRole::VertexFlags).unwrap_or_default(),
            crate::jt_topology::SplitLanes {
                faces: values(TopologyPacketRole::SplitFaceSymbols).unwrap_or_default(),
                positions: values(TopologyPacketRole::SplitFacePositions).unwrap_or_default(),
            },
            crate::jt_topology::AttributeMaskLanes {
                small: attribute_masks,
                context_7_next_30,
                context_7_upper_4,
                large_words: &large_words,
            },
        )?
        else {
            return Ok(Vec::new());
        };
        if polygons.iter().any(|polygon| {
            polygon
                .corners
                .iter()
                .any(|&(index, _)| index >= coordinate_header.unique_vertex_count)
        }) {
            return Ok(Vec::new());
        }
        ctx.reserve_record_vec(&mut meshes, 1, 0, "nx JT polygon meshes")?;
        let mesh = DisplayJtPolygonMesh {
            id: replace_jt_text_once(
                ctx,
                &sequence.id,
                "topology-packets",
                "polygon-mesh",
                "nx JT polygon mesh identity",
            )?,
            topology: ctx.join_retained(&[&sequence.id], "", "nx JT topology reference")?,
            coordinate_header: ctx.join_retained(
                &[&coordinate_header.id],
                "",
                "nx JT coordinate reference",
            )?,
            polygons,
            source_offset: sequence.source_offset,
        };
        meshes.push(mesh);
    }
    Ok(meshes)
}

/// Decode every complete JT 9 normal array following a coordinate array.
pub(super) fn display_jt_vertex_normals(
    ctx: &DecodeContext<'_>,
    container: &Container,
    vertex_headers: &[DisplayJtCompressedVertexRecordsHeader],
    coordinate_headers: &[DisplayJtVertexCoordinateArrayHeader],
    coordinates: &[DisplayJtVertexCoordinates],
) -> Result<Vec<DisplayJtVertexNormals>, CodecError> {
    let mut arrays = Vec::new();
    for vertex_header in vertex_headers {
        if vertex_header.vertex_attribute_count == 0 || vertex_header.vertex_bindings & 0x8 == 0 {
            continue;
        }
        let Some(coordinate_header) = coordinate_headers
            .iter()
            .find(|header| header.element == vertex_header.element)
        else {
            return Ok(Vec::new());
        };
        let Some(coordinates) = coordinates
            .iter()
            .find(|coordinates| coordinates.header == coordinate_header.id)
        else {
            return Ok(Vec::new());
        };
        let Some(source_offset) = coordinates
            .source_offset
            .checked_add(u64::from(coordinates.byte_len))
        else {
            return Ok(Vec::new());
        };
        let Some(bytes) = container.bounded_entry_tail(source_offset) else {
            return Ok(Vec::new());
        };
        let Some(crate::jt::DecodedVertexArray {
            values: normals,
            hash: normal_hash,
            byte_len,
        }) = crate::jt::decode_vertex_normals(
            ctx,
            bytes,
            cadmpeg_core::decode::index_from_u32(vertex_header.vertex_attribute_count),
            vertex_header.normal_quantization_factor,
        )?
        else {
            return Ok(Vec::new());
        };
        let Ok(byte_len) = u32::try_from(byte_len) else {
            return Ok(Vec::new());
        };
        ctx.reserve_record_vec(&mut arrays, 1, 0, "nx JT vertex normals")?;
        arrays.push(DisplayJtVertexNormals {
            id: ctx.join_retained(
                &[&vertex_header.element, "-vertex-normals"],
                "",
                "nx JT normal identity",
            )?,
            vertex_records_header: ctx.join_retained(
                &[&vertex_header.id],
                "",
                "nx JT normal header reference",
            )?,
            normals,
            normal_hash,
            byte_len,
            source_offset,
        });
    }
    Ok(arrays)
}

/// Decode every complete JT 9 color array after coordinates and optional normals.
pub(super) fn display_jt_vertex_colors(
    ctx: &DecodeContext<'_>,
    container: &Container,
    vertex_headers: &[DisplayJtCompressedVertexRecordsHeader],
    coordinate_headers: &[DisplayJtVertexCoordinateArrayHeader],
    coordinates: &[DisplayJtVertexCoordinates],
    normals: &[DisplayJtVertexNormals],
) -> Result<Vec<DisplayJtVertexColors>, CodecError> {
    let mut arrays = Vec::new();
    for vertex_header in vertex_headers {
        if vertex_header.vertex_attribute_count == 0 || vertex_header.vertex_bindings & 0x30 == 0 {
            continue;
        }
        let Some(coordinate_header) = coordinate_headers
            .iter()
            .find(|header| header.element == vertex_header.element)
        else {
            return Ok(Vec::new());
        };
        let Some(coordinates) = coordinates
            .iter()
            .find(|coordinates| coordinates.header == coordinate_header.id)
        else {
            return Ok(Vec::new());
        };
        let Some(mut source_offset) = coordinates
            .source_offset
            .checked_add(u64::from(coordinates.byte_len))
        else {
            return Ok(Vec::new());
        };
        if vertex_header.vertex_bindings & 0x8 != 0 {
            let Some(normal_array) = normals
                .iter()
                .find(|normal| normal.vertex_records_header == vertex_header.id)
            else {
                return Ok(Vec::new());
            };
            let Some(next) = source_offset.checked_add(u64::from(normal_array.byte_len)) else {
                return Ok(Vec::new());
            };
            source_offset = next;
        }
        let Some(bytes) = container.bounded_entry_tail(source_offset) else {
            return Ok(Vec::new());
        };
        let Some(crate::jt::DecodedVertexArray {
            values: colors,
            hash: color_hash,
            byte_len,
        }) = crate::jt::decode_vertex_colors(
            ctx,
            bytes,
            cadmpeg_core::decode::index_from_u32(vertex_header.vertex_attribute_count),
            vertex_header.color_quantization_bits,
        )?
        else {
            return Ok(Vec::new());
        };
        let Ok(byte_len) = u32::try_from(byte_len) else {
            return Ok(Vec::new());
        };
        ctx.reserve_record_vec(&mut arrays, 1, 0, "nx JT vertex colors")?;
        arrays.push(DisplayJtVertexColors {
            id: ctx.join_retained(
                &[&vertex_header.element, "-vertex-colors"],
                "",
                "nx JT color identity",
            )?,
            vertex_records_header: ctx.join_retained(
                &[&vertex_header.id],
                "",
                "nx JT color header reference",
            )?,
            colors,
            color_hash,
            byte_len,
            source_offset,
        });
    }
    Ok(arrays)
}

/// Decode texture-coordinate channels after preceding coordinate, normal, and color arrays.
pub(super) fn display_jt_vertex_texture_coordinates(
    ctx: &DecodeContext<'_>,
    container: &Container,
    vertex_headers: &[DisplayJtCompressedVertexRecordsHeader],
    coordinate_headers: &[DisplayJtVertexCoordinateArrayHeader],
    coordinates: &[DisplayJtVertexCoordinates],
    normals: &[DisplayJtVertexNormals],
    colors: &[DisplayJtVertexColors],
) -> Result<Vec<DisplayJtVertexTextureCoordinates>, CodecError> {
    let mut arrays = Vec::new();
    for vertex_header in vertex_headers {
        if !(0..8)
            .any(|channel| vertex_header.vertex_bindings & (0xf_u64 << (8 + 4 * channel)) != 0)
        {
            continue;
        }
        if vertex_header.vertex_attribute_count == 0 {
            return Ok(Vec::new());
        }
        let Some(coordinate_header) = coordinate_headers
            .iter()
            .find(|header| header.element == vertex_header.element)
        else {
            return Ok(Vec::new());
        };
        let Some(coordinates) = coordinates
            .iter()
            .find(|coordinates| coordinates.header == coordinate_header.id)
        else {
            return Ok(Vec::new());
        };
        let Some(mut source_offset) = coordinates
            .source_offset
            .checked_add(u64::from(coordinates.byte_len))
        else {
            return Ok(Vec::new());
        };
        if vertex_header.vertex_bindings & 0x8 != 0 {
            let Some(normal_array) = normals
                .iter()
                .find(|normal| normal.vertex_records_header == vertex_header.id)
            else {
                return Ok(Vec::new());
            };
            let Some(next) = source_offset.checked_add(u64::from(normal_array.byte_len)) else {
                return Ok(Vec::new());
            };
            source_offset = next;
        }
        if vertex_header.vertex_bindings & 0x30 != 0 {
            let Some(color_array) = colors
                .iter()
                .find(|color| color.vertex_records_header == vertex_header.id)
            else {
                return Ok(Vec::new());
            };
            let Some(next) = source_offset.checked_add(u64::from(color_array.byte_len)) else {
                return Ok(Vec::new());
            };
            source_offset = next;
        }
        for channel in (0..8)
            .filter(|channel| vertex_header.vertex_bindings & (0xf_u64 << (8 + 4 * channel)) != 0)
        {
            let Some(bytes) = container.bounded_entry_tail(source_offset) else {
                return Ok(Vec::new());
            };
            let Some(crate::jt::DecodedVertexArray {
                values,
                hash: texture_coordinate_hash,
                byte_len,
            }) = crate::jt::decode_vertex_texture_coordinates(
                ctx,
                bytes,
                cadmpeg_core::decode::index_from_u32(vertex_header.vertex_attribute_count),
                vertex_header.texture_quantization_bits,
            )?
            else {
                return Ok(Vec::new());
            };
            let Ok(byte_len) = u32::try_from(byte_len) else {
                return Ok(Vec::new());
            };
            ctx.reserve_record_vec(&mut arrays, 1, 0, "nx JT texture coordinates")?;
            arrays.push(DisplayJtVertexTextureCoordinates {
                id: ctx.join_retained(
                    &[
                        &vertex_header.element,
                        "-texture-coordinates-",
                        ["0", "1", "2", "3", "4", "5", "6", "7"][channel],
                    ],
                    "",
                    "nx JT texture identity",
                )?,
                vertex_records_header: ctx.join_retained(
                    &[&vertex_header.id],
                    "",
                    "nx JT texture header reference",
                )?,
                channel: u8::try_from(channel)
                    .map_err(|_| CodecError::malformed("DisplayJT texture channel exceeds u8"))?,
                values,
                texture_coordinate_hash,
                byte_len,
                source_offset,
            });
            let Some(next) = source_offset.checked_add(u64::from(byte_len)) else {
                return Ok(Vec::new());
            };
            source_offset = next;
        }
    }
    Ok(arrays)
}

/// Decode every complete JT 9 vertex-flag array after all preceding vertex arrays.
#[derive(Clone, Copy)]
pub(super) struct DisplayJtVertexFlagInputs<'a, 'b> {
    pub(super) container: &'a Container<'b>,
    pub(super) vertex_headers: &'a [DisplayJtCompressedVertexRecordsHeader],
    pub(super) coordinate_headers: &'a [DisplayJtVertexCoordinateArrayHeader],
    pub(super) coordinates: &'a [DisplayJtVertexCoordinates],
    pub(super) normals: &'a [DisplayJtVertexNormals],
    pub(super) colors: &'a [DisplayJtVertexColors],
    pub(super) texture_coordinates: &'a [DisplayJtVertexTextureCoordinates],
}

pub(super) fn display_jt_vertex_flags(
    ctx: &DecodeContext<'_>,
    inputs: DisplayJtVertexFlagInputs<'_, '_>,
) -> Result<Vec<DisplayJtVertexFlags>, CodecError> {
    let DisplayJtVertexFlagInputs {
        container,
        vertex_headers,
        coordinate_headers,
        coordinates,
        normals,
        colors,
        texture_coordinates,
    } = inputs;
    let mut arrays = Vec::new();
    for vertex_header in vertex_headers {
        if vertex_header.vertex_attribute_count == 0 || vertex_header.vertex_bindings & 0x40 == 0 {
            continue;
        }
        let Some(coordinate_header) = coordinate_headers
            .iter()
            .find(|header| header.element == vertex_header.element)
        else {
            return Ok(Vec::new());
        };
        let Some(coordinates) = coordinates
            .iter()
            .find(|coordinates| coordinates.header == coordinate_header.id)
        else {
            return Ok(Vec::new());
        };
        let Some(mut source_offset) = coordinates
            .source_offset
            .checked_add(u64::from(coordinates.byte_len))
        else {
            return Ok(Vec::new());
        };
        if vertex_header.vertex_bindings & 0x8 != 0 {
            let Some(array) = normals
                .iter()
                .find(|array| array.vertex_records_header == vertex_header.id)
            else {
                return Ok(Vec::new());
            };
            let Some(next) = source_offset.checked_add(u64::from(array.byte_len)) else {
                return Ok(Vec::new());
            };
            source_offset = next;
        }
        if vertex_header.vertex_bindings & 0x30 != 0 {
            let Some(array) = colors
                .iter()
                .find(|array| array.vertex_records_header == vertex_header.id)
            else {
                return Ok(Vec::new());
            };
            let Some(next) = source_offset.checked_add(u64::from(array.byte_len)) else {
                return Ok(Vec::new());
            };
            source_offset = next;
        }
        for channel in (0..8)
            .filter(|channel| vertex_header.vertex_bindings & (0xf_u64 << (8 + 4 * channel)) != 0)
        {
            let Some(array) = texture_coordinates.iter().find(|array| {
                array.vertex_records_header == vertex_header.id
                    && usize::from(array.channel) == channel
            }) else {
                return Ok(Vec::new());
            };
            let Some(next) = source_offset.checked_add(u64::from(array.byte_len)) else {
                return Ok(Vec::new());
            };
            source_offset = next;
        }
        let Some(bytes) = container.bounded_entry_tail(source_offset) else {
            return Ok(Vec::new());
        };
        let Some((values, byte_len)) = crate::jt::decode_vertex_flags(
            ctx,
            bytes,
            cadmpeg_core::decode::index_from_u32(vertex_header.vertex_attribute_count),
        )?
        else {
            return Ok(Vec::new());
        };
        let Ok(byte_len) = u32::try_from(byte_len) else {
            return Ok(Vec::new());
        };
        ctx.reserve_record_vec(&mut arrays, 1, 0, "nx JT vertex flags")?;
        arrays.push(DisplayJtVertexFlags {
            id: ctx.join_retained(
                &[&vertex_header.element, "-vertex-flags"],
                "",
                "nx JT flag identity",
            )?,
            vertex_records_header: ctx.join_retained(
                &[&vertex_header.id],
                "",
                "nx JT flag header reference",
            )?,
            values,
            byte_len,
            source_offset,
        });
    }
    Ok(arrays)
}

/// Decode element framing and exact post-marker tails from compressed segments.
pub(super) fn display_jt_compressed_element_sequences(
    budget: (&DecodeContext<'_>, View<'_>),
    container: &Container,
    segments: &[DisplayJtSegment],
) -> Result<
    (
        Vec<DisplayJtCompressedElement>,
        Vec<DisplayJtCompressedElementSequence>,
    ),
    cadmpeg_core::CodecError,
> {
    let mut elements = Vec::new();
    let mut sequences = Vec::new();
    for segment in segments
        .iter()
        .filter(|segment| segment.compression.is_some())
    {
        let Some(bytes) = container
            .bounded_entry_bytes(segment.source_offset, u64::from(segment.segment_byte_len))
        else {
            return Ok((Vec::new(), Vec::new()));
        };
        let Some(compressed) = bytes.get(33..) else {
            return Ok((Vec::new(), Vec::new()));
        };
        let Some(member) = segment
            .source_offset
            .checked_add(33)
            .and_then(cadmpeg_core::decode::index_from_u64)
            .and_then(|start| {
                start
                    .checked_add(compressed.len())
                    .and_then(|end| budget.1.child(start, end))
            })
        else {
            return Ok((Vec::new(), Vec::new()));
        };
        let Some(inflated) = inflate_display_jt(budget.0, member)? else {
            return Ok((Vec::new(), Vec::new()));
        };
        let Some((parsed, framed_end)) = parse_jt_element_sequence(budget.0, &inflated)? else {
            return Ok((Vec::new(), Vec::new()));
        };
        {
            let ctx = budget.0;
            let count = u64::try_from(parsed.len()).map_err(|_| {
                ctx.refuse_codec_limit("count DisplayJT compressed elements", 0, u64::MAX)
            })?;
            let id_slots = count
                .checked_mul(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                    String,
                >()))
                .ok_or_else(|| ctx.refuse_codec_limit("size DisplayJT element ids", 0, count))?;
            let element_slots = count
                .checked_mul(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                    DisplayJtCompressedElement,
                >()))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("size DisplayJT compressed elements", 0, count)
                })?;
            ctx.charge_collection_items(count, "store DisplayJT element ids")?;
            ctx.charge_retained(id_slots, "retain DisplayJT element ids")?;
            ctx.charge_collection_items(count, "store DisplayJT compressed elements")?;
            ctx.charge_retained(element_slots, "retain DisplayJT compressed elements")?;
        }
        let mut element_ids = Vec::new();
        if cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut element_ids,
            parsed.len(),
            "allocate DisplayJT element ids",
        )
        .is_err()
        {
            return Err(budget
                .0
                .refuse_codec_limit("allocate DisplayJT element ids", 0, 1));
        }
        if cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut elements,
            parsed.len(),
            "allocate DisplayJT compressed elements",
        )
        .is_err()
        {
            return Err(budget.0.refuse_codec_limit(
                "allocate DisplayJT compressed elements",
                0,
                1,
            ));
        }
        for (ordinal, element) in parsed.into_iter().enumerate() {
            {
                let ctx = budget.0;
                let digits = if ordinal == 0 {
                    1
                } else {
                    cadmpeg_core::decode::index_from_u32(ordinal.ilog10()) + 1
                };
                let id_len = segment
                    .id
                    .len()
                    .checked_add("-inflated-element-".len())
                    .and_then(|len| len.checked_add(digits))
                    .ok_or_else(|| {
                        ctx.refuse_codec_limit("size DisplayJT element identity", 0, 1)
                    })?;
                let string_bytes = id_len
                    .checked_mul(2)
                    .and_then(|len| len.checked_add(segment.id.len()))
                    .and_then(|len| len.checked_add(64))
                    .and_then(|len| u64::try_from(len).ok())
                    .ok_or_else(|| ctx.refuse_codec_limit("size DisplayJT element fields", 0, 1))?;
                ctx.charge_retained(string_bytes, "retain DisplayJT compressed element fields")?;
                let body_work = u64::try_from(element.body.len()).map_err(|_| {
                    ctx.refuse_codec_limit("size DisplayJT compressed element body", 0, u64::MAX)
                })?;
                ctx.charge_work(body_work, "hash DisplayJT compressed element body")?;
            }
            let id = format!("{}-inflated-element-{ordinal}", segment.id);
            element_ids.push(id.clone());
            elements.push(
                DisplayJtCompressedElement::try_from(DisplayJtCompressedElementWire {
                    id,
                    segment: segment.id.clone(),
                    segment_type: segment.segment_type,
                    ordinal: u32::try_from(ordinal)
                        .map_err(|_| display_jt_framing_error("ordinal exceeds u32"))?,
                    object_type_id: element.object_type_id,
                    object_id: element.object_id,
                    object_base_type: element.object_base_type,
                    body_byte_len: u32::try_from(element.body.len())
                        .map_err(|_| display_jt_framing_error("body_byte_len exceeds u32"))?,
                    body_sha256: Sha256Digest::digest(element.body),
                    inflated_offset: u32::try_from(element.offset)
                        .map_err(|_| display_jt_framing_error("inflated_offset exceeds u32"))?,
                    source_offset: segment.source_offset + 24,
                })
                .map_err(display_jt_framing_error)?,
            );
        }
        let tail = &inflated[framed_end..];
        {
            let ctx = budget.0;
            ctx.charge_collection_items(1, "store DisplayJT compressed sequence")?;
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                    DisplayJtCompressedElementSequence,
                >()),
                "retain DisplayJT compressed sequence",
            )?;
            let string_bytes = segment
                .id
                .len()
                .checked_mul(2)
                .and_then(|len| len.checked_add("-inflated-sequence".len()))
                .and_then(|len| len.checked_add(64))
                .and_then(|len| u64::try_from(len).ok())
                .ok_or_else(|| ctx.refuse_codec_limit("size DisplayJT sequence fields", 0, 1))?;
            ctx.charge_retained(string_bytes, "retain DisplayJT compressed sequence fields")?;
            let tail_work = u64::try_from(tail.len())
                .map_err(|_| ctx.refuse_codec_limit("size DisplayJT sequence tail", 0, u64::MAX))?;
            ctx.charge_work(tail_work, "hash DisplayJT compressed sequence tail")?;
        }
        let ctx = budget.0;
        cadmpeg_core::decode::DecodeContext::reserve_admitted_vec(
            &mut sequences,
            1,
            "allocate DisplayJT compressed sequence",
        )?;
        let retained_tail = ctx.copy_retained(tail, "retain DisplayJT compressed sequence tail")?;
        let tail_work = cadmpeg_core::decode::u64_from_index(tail.len());
        ctx.charge_work(tail_work, "check DisplayJT compressed sequence tail hash")?;
        let _digest_check = ctx.reserve_scoped(64, "check DisplayJT sequence tail hash")?;
        sequences.push(
            DisplayJtCompressedElementSequence::try_from(DisplayJtCompressedElementSequenceWire {
                id: format!("{}-inflated-sequence", segment.id),
                segment: segment.id.clone(),
                segment_type: segment.segment_type,
                elements: element_ids,
                framed_byte_len: u32::try_from(framed_end)
                    .map_err(|_| display_jt_framing_error("framed_byte_len exceeds u32"))?,
                tail: retained_tail,
                tail_sha256: Sha256Digest::digest(tail),
                source_offset: segment.source_offset + 24,
            })
            .map_err(display_jt_framing_error)?,
        );
    }
    Ok((elements, sequences))
}

fn display_jt_framing_error(message: &str) -> cadmpeg_core::CodecError {
    cadmpeg_core::CodecError::malformed(format!(
        "{}: {message}",
        crate::loss::NxLossCode::DisplayJtGraphRejected.code()
    ))
}

/// Decode all string property atoms from complete type-31 segment sequences.
pub(super) fn display_jt_string_property_atoms(
    budget: (&DecodeContext<'_>, View<'_>),
    container: &Container,
    segments: &[DisplayJtSegment],
) -> Result<Vec<DisplayJtStringPropertyAtom>, CodecError> {
    const STRING_PROPERTY_ATOM_TYPE: [u8; 16] = [
        0x6e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut atoms = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 31) {
        if segment.compression.is_none() {
            return Ok(Vec::new());
        }
        let Some(bytes) = container
            .bounded_entry_bytes(segment.source_offset, u64::from(segment.segment_byte_len))
        else {
            return Ok(Vec::new());
        };
        let Some(compressed) = bytes.get(33..) else {
            return Ok(Vec::new());
        };
        let Some(member) = segment
            .source_offset
            .checked_add(33)
            .and_then(cadmpeg_core::decode::index_from_u64)
            .and_then(|start| {
                start
                    .checked_add(compressed.len())
                    .and_then(|end| budget.1.child(start, end))
            })
        else {
            return Ok(Vec::new());
        };
        let Some(inflated) = inflate_display_jt(budget.0, member)? else {
            return Ok(Vec::new());
        };
        let Some((elements, _)) = parse_jt_element_sequence(budget.0, &inflated)? else {
            return Ok(Vec::new());
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_type_id != STRING_PROPERTY_ATOM_TYPE || element.object_base_type != 5
            {
                return Ok(Vec::new());
            }
            let Some(value) = parse_jt_string_property_atom_body(budget.0, element.body)? else {
                return Ok(Vec::new());
            };
            budget.0.reserve_record_vec(
                &mut atoms,
                1,
                display_jt_text_size(
                    budget.0,
                    &[
                        &segment.id,
                        "-string-property-atom-",
                        &segment.id,
                        "-inflated-element-",
                    ],
                    decimal_digits(ordinal).checked_mul(2).ok_or_else(|| {
                        budget.0.refuse_codec_limit(
                            "store DisplayJT string property atom",
                            0,
                            u64::MAX,
                        )
                    })?,
                )?,
                "store DisplayJT string property atom",
            )?;
            atoms.push(DisplayJtStringPropertyAtom {
                id: format!("{}-string-property-atom-{ordinal}", segment.id),
                element: format!("{}-inflated-element-{ordinal}", segment.id),
                object_id: element.object_id,
                value,
                source_offset: segment.source_offset + 24,
            });
        }
    }
    Ok(atoms)
}
/// Resolve JT 9 logical shape nodes to their late-loaded type-7 LOD segments.
pub(super) fn display_jt_shape_lod_bindings(
    budget: (&DecodeContext<'_>, View<'_>),
    container: &Container,
    segments: &[DisplayJtSegment],
) -> Result<Vec<DisplayJtShapeLodBinding>, CodecError> {
    const STRING_PROPERTY_ATOM_TYPE: [u8; 16] = [
        0x6e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    const LATE_LOADED_PROPERTY_ATOM_TYPE: [u8; 16] = [
        0xe5, 0x5b, 0xb0, 0xe0, 0xbd, 0xfb, 0xd1, 0x11, 0xa3, 0xa7, 0x00, 0xaa, 0x00, 0xd1, 0x09,
        0x54,
    ];
    const SHAPE_IMPLEMENTATION_KEY: &str = "JT_LLPROP_SHAPEIMPL";
    let mut bindings = Vec::new();
    for scene_segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(bytes) = container.bounded_entry_bytes(
            scene_segment.source_offset,
            u64::from(scene_segment.segment_byte_len),
        ) else {
            return Ok(Vec::new());
        };
        let Some(compressed) = bytes.get(33..) else {
            return Ok(Vec::new());
        };
        let Some(member) = scene_segment
            .source_offset
            .checked_add(33)
            .and_then(cadmpeg_core::decode::index_from_u64)
            .and_then(|start| {
                start
                    .checked_add(compressed.len())
                    .and_then(|end| budget.1.child(start, end))
            })
        else {
            return Ok(Vec::new());
        };
        let Some(inflated) = inflate_display_jt(budget.0, member)? else {
            return Ok(Vec::new());
        };
        let Some((_, scene_end)) = parse_jt_element_sequence(budget.0, &inflated)? else {
            return Ok(Vec::new());
        };
        let tail = &inflated[scene_end..];
        let Some((property_atoms, property_table_offset)) =
            parse_jt_element_sequence(budget.0, tail)?
        else {
            return Ok(Vec::new());
        };
        let _map_reservation = {
            let ctx = budget.0;
            let count = property_atoms
                .iter()
                .filter(|atom| {
                    (atom.object_type_id == STRING_PROPERTY_ATOM_TYPE && atom.object_base_type == 5)
                        || (atom.object_type_id == LATE_LOADED_PROPERTY_ATOM_TYPE
                            && atom.object_base_type == 8)
                })
                .count();
            let count = u64::try_from(count).map_err(|_| {
                ctx.refuse_codec_limit("store DisplayJT property atoms", 0, u64::MAX)
            })?;
            ctx.charge_collection_items(count, "store DisplayJT property atoms")?;
            let bytes = count.checked_mul(128).ok_or_else(|| {
                ctx.refuse_codec_limit("store DisplayJT property atoms", 0, u64::MAX)
            })?;
            ctx.reserve_scoped(bytes, "store DisplayJT property atoms")?
        };
        let mut strings = BTreeMap::new();
        let mut late_loaded = BTreeMap::new();
        for atom in property_atoms {
            if atom.object_type_id == STRING_PROPERTY_ATOM_TYPE && atom.object_base_type == 5 {
                let Some(value) = parse_jt_string_property_atom_body(budget.0, atom.body)? else {
                    return Ok(Vec::new());
                };
                strings.insert(atom.object_id, value);
            } else if atom.object_type_id == LATE_LOADED_PROPERTY_ATOM_TYPE
                && atom.object_base_type == 8
            {
                if atom.body.len() != 36 || View::u16_le_at(atom.body, 0) != Some(1) {
                    return Ok(Vec::new());
                }
                let Some(state_flags) = View::u32_le_at(atom.body, 2) else {
                    return Ok(Vec::new());
                };
                let Some(property_version) = View::u16_le_at(atom.body, 6) else {
                    return Ok(Vec::new());
                };
                let Ok(segment_id) = <[u8; 16]>::try_from(&atom.body[8..24]) else {
                    return Ok(Vec::new());
                };
                let Some(segment_type) = View::u32_le_at(atom.body, 24) else {
                    return Ok(Vec::new());
                };
                let Some(payload_object_id) = View::u32_le_at(atom.body, 28) else {
                    return Ok(Vec::new());
                };
                let Some(reserved_value) =
                    View::u32_le_at(atom.body, 32).filter(|value| *value != 0)
                else {
                    return Ok(Vec::new());
                };
                late_loaded.insert(
                    atom.object_id,
                    (
                        state_flags,
                        property_version,
                        segment_id,
                        segment_type,
                        payload_object_id,
                        reserved_value,
                    ),
                );
            }
        }
        let table = &tail[property_table_offset..];
        let mut table_view = View::over_retained(table);
        let Some(table_version) = table_view.u16_le() else {
            return Ok(Vec::new());
        };
        let Some(table_count) = table_view.u32_le() else {
            return Ok(Vec::new());
        };
        for table_ordinal in 0..table_count {
            let Some(shape_node_object_id) = table_view.u32_le() else {
                return Ok(Vec::new());
            };
            let mut pair_ordinal = 0u32;
            loop {
                let Some(key_object_id) = table_view.u32_le() else {
                    return Ok(Vec::new());
                };
                if key_object_id == 0 {
                    break;
                }
                let Some(value_object_id) = table_view.u32_le() else {
                    return Ok(Vec::new());
                };
                if strings.get(&key_object_id).map(String::as_str) == Some(SHAPE_IMPLEMENTATION_KEY)
                {
                    let Some((
                        state_flags,
                        property_version,
                        segment_id,
                        segment_type,
                        payload_object_id,
                        reserved_value,
                    )) = late_loaded.get(&value_object_id)
                    else {
                        return Ok(Vec::new());
                    };
                    let mut targets = segments.iter().filter(|segment| {
                        segment.document == scene_segment.document
                            && segment.segment_id == *segment_id
                            && segment.segment_type == *segment_type
                    });
                    let Some(target) = targets.next() else {
                        return Ok(Vec::new());
                    };
                    if targets.next().is_some() || target.segment_type != 7 {
                        return Ok(Vec::new());
                    }
                    let ctx = budget.0;
                    let text_bytes = display_jt_text_size(
                        ctx,
                        &[
                            &scene_segment.id,
                            "-shape-lod-binding--",
                            &scene_segment.id,
                            SHAPE_IMPLEMENTATION_KEY,
                            &target.id,
                        ],
                        decimal_digits(cadmpeg_core::decode::index_from_u32(table_ordinal))
                            + decimal_digits(cadmpeg_core::decode::index_from_u32(pair_ordinal)),
                    )?;
                    ctx.reserve_record_vec(
                        &mut bindings,
                        1,
                        text_bytes,
                        "store DisplayJT shape LOD binding",
                    )?;
                    bindings.push(DisplayJtShapeLodBinding {
                        id: format!(
                            "{}-shape-lod-binding-{table_ordinal}-{pair_ordinal}",
                            scene_segment.id
                        ),
                        scene_segment: scene_segment.id.clone(),
                        table_version,
                        shape_node_object_id,
                        key_object_id,
                        key: SHAPE_IMPLEMENTATION_KEY.to_string(),
                        value_object_id,
                        state_flags: *state_flags,
                        property_version: *property_version,
                        shape_segment: target.id.clone(),
                        payload_object_id: *payload_object_id,
                        reserved_value: *reserved_value,
                        source_offset: scene_segment.source_offset + 24,
                    });
                }
                pair_ordinal += 1;
            }
        }
        if !table_view.is_empty() {
            return Ok(Vec::new());
        }
    }
    Ok(bindings)
}
/// Decode the common node-data header from every type-1 segment element.
pub(super) fn display_jt_base_node_data(
    budget: (&DecodeContext<'_>, View<'_>),
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Result<Vec<DisplayJtBaseNodeData>, CodecError> {
    let mut nodes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Ok(Vec::new());
        };
        if segment.compression.is_none() {
            return Ok(Vec::new());
        }
        let Some(bytes) = container
            .bounded_entry_bytes(segment.source_offset, u64::from(segment.segment_byte_len))
        else {
            return Ok(Vec::new());
        };
        let Some(compressed) = bytes.get(33..) else {
            return Ok(Vec::new());
        };
        let Some(member) = segment
            .source_offset
            .checked_add(33)
            .and_then(cadmpeg_core::decode::index_from_u64)
            .and_then(|start| {
                start
                    .checked_add(compressed.len())
                    .and_then(|end| budget.1.child(start, end))
            })
        else {
            return Ok(Vec::new());
        };
        let Some(inflated) = inflate_display_jt(budget.0, member)? else {
            return Ok(Vec::new());
        };
        let Some((elements, _)) = parse_jt_element_sequence(budget.0, &inflated)? else {
            return Ok(Vec::new());
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_base_type > 2 {
                continue;
            }
            let Some((_, _base_reservation)) =
                admit_jt_base_body(budget.0, element.body, document.version.major(), true)?
            else {
                return Ok(Vec::new());
            };
            let Some((version, flags, attribute_object_ids, family_data)) =
                parse_jt_base_node_body(element.body, document.version.major())
            else {
                return Ok(Vec::new());
            };
            budget.0.reserve_record_vec(
                &mut nodes,
                1,
                display_jt_text_size(
                    budget.0,
                    &[
                        &segment.id,
                        "-base-node-",
                        &segment.id,
                        "-inflated-element-",
                    ],
                    decimal_digits(ordinal).checked_mul(2).ok_or_else(|| {
                        budget
                            .0
                            .refuse_codec_limit("store DisplayJT base node", 0, u64::MAX)
                    })?,
                )?,
                "store DisplayJT base node",
            )?;
            nodes.push(DisplayJtBaseNodeData {
                id: format!("{}-base-node-{ordinal}", segment.id),
                element: format!("{}-inflated-element-{ordinal}", segment.id),
                object_type_id: element.object_type_id,
                object_id: element.object_id,
                version,
                flags,
                attribute_object_ids,
                family_data_byte_len: u32::try_from(family_data.len()).map_err(|_| {
                    budget.0.refuse_codec_limit(
                        "DisplayJT count exceeds u32",
                        u64::from(u32::MAX),
                        cadmpeg_core::decode::u64_from_index(family_data.len()),
                    )
                })?,
                family_data_sha256: Sha256Digest::digest_for_decode(
                    budget.0,
                    family_data,
                    "retain DisplayJT hash",
                )?,
                source_offset: segment.source_offset + 24,
            });
        }
    }
    Ok(nodes)
}
/// Decode common group-node data from every JT 9 group-derived scene node.
pub(super) fn display_jt_group_node_data(
    budget: (&DecodeContext<'_>, View<'_>),
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Result<Vec<DisplayJtGroupNodeData>, CodecError> {
    let mut nodes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Ok(Vec::new());
        };
        if document.version.major() != 9 || segment.compression.is_none() {
            continue;
        }
        let Some(bytes) = container
            .bounded_entry_bytes(segment.source_offset, u64::from(segment.segment_byte_len))
        else {
            return Ok(Vec::new());
        };
        let Some(compressed) = bytes.get(33..) else {
            return Ok(Vec::new());
        };
        let Some(member) = segment
            .source_offset
            .checked_add(33)
            .and_then(cadmpeg_core::decode::index_from_u64)
            .and_then(|start| {
                start
                    .checked_add(compressed.len())
                    .and_then(|end| budget.1.child(start, end))
            })
        else {
            return Ok(Vec::new());
        };
        let Some(inflated) = inflate_display_jt(budget.0, member)? else {
            return Ok(Vec::new());
        };
        let Some((elements, _)) = parse_jt_element_sequence(budget.0, &inflated)? else {
            return Ok(Vec::new());
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_base_type != 1 {
                continue;
            }
            let Some((_, _base_reservation)) = admit_jt_group_body(budget.0, element.body)? else {
                return Ok(Vec::new());
            };
            let Some((version, child_object_ids, family_data)) =
                parse_jt9_group_node_body(element.body)
            else {
                return Ok(Vec::new());
            };
            if version != 1 {
                return Ok(Vec::new());
            }
            budget.0.reserve_record_vec(
                &mut nodes,
                1,
                display_jt_text_size(
                    budget.0,
                    &[&segment.id, "-group-node-data-", &segment.id, "-base-node-"],
                    decimal_digits(ordinal).checked_mul(2).ok_or_else(|| {
                        budget
                            .0
                            .refuse_codec_limit("store DisplayJT group node", 0, u64::MAX)
                    })?,
                )?,
                "store DisplayJT group node",
            )?;
            nodes.push(DisplayJtGroupNodeData {
                id: format!("{}-group-node-data-{ordinal}", segment.id),
                base_node: format!("{}-base-node-{ordinal}", segment.id),
                object_id: element.object_id,
                version,
                child_object_ids,
                family_data_byte_len: u32::try_from(family_data.len()).map_err(|_| {
                    budget.0.refuse_codec_limit(
                        "DisplayJT count exceeds u32",
                        u64::from(u32::MAX),
                        cadmpeg_core::decode::u64_from_index(family_data.len()),
                    )
                })?,
                family_data_sha256: Sha256Digest::digest_for_decode(
                    budget.0,
                    family_data,
                    "retain DisplayJT hash",
                )?,
                source_offset: segment.source_offset + 24,
            });
        }
    }
    Ok(nodes)
}
/// Decode complete JT 9 instance nodes from logical scene-graph segments.
pub(super) fn display_jt_instance_nodes(
    budget: (&DecodeContext<'_>, View<'_>),
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Result<Vec<DisplayJtInstanceNode>, CodecError> {
    const INSTANCE_NODE_TYPE: [u8; 16] = [
        0x2a, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut nodes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Ok(Vec::new());
        };
        if document.version.major() != 9 || segment.compression.is_none() {
            continue;
        }
        let Some(bytes) = container
            .bounded_entry_bytes(segment.source_offset, u64::from(segment.segment_byte_len))
        else {
            return Ok(Vec::new());
        };
        let Some(compressed) = bytes.get(33..) else {
            return Ok(Vec::new());
        };
        let Some(member) = segment
            .source_offset
            .checked_add(33)
            .and_then(cadmpeg_core::decode::index_from_u64)
            .and_then(|start| {
                start
                    .checked_add(compressed.len())
                    .and_then(|end| budget.1.child(start, end))
            })
        else {
            return Ok(Vec::new());
        };
        let Some(inflated) = inflate_display_jt(budget.0, member)? else {
            return Ok(Vec::new());
        };
        let Some((elements, _)) = parse_jt_element_sequence(budget.0, &inflated)? else {
            return Ok(Vec::new());
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_type_id != INSTANCE_NODE_TYPE {
                continue;
            }
            if element.object_base_type != 0 {
                return Ok(Vec::new());
            }
            let Some((_, _base_reservation)) =
                admit_jt_base_body(budget.0, element.body, 9, false)?
            else {
                return Ok(Vec::new());
            };
            let Some((version, child_object_id)) = parse_jt9_instance_node_body(element.body)
            else {
                return Ok(Vec::new());
            };
            budget.0.reserve_record_vec(
                &mut nodes,
                1,
                display_jt_text_size(
                    budget.0,
                    &[&segment.id, "-instance-node-", &segment.id, "-base-node-"],
                    decimal_digits(ordinal).checked_mul(2).ok_or_else(|| {
                        budget
                            .0
                            .refuse_codec_limit("store DisplayJT instance node", 0, u64::MAX)
                    })?,
                )?,
                "store DisplayJT instance node",
            )?;
            nodes.push(DisplayJtInstanceNode {
                id: format!("{}-instance-node-{ordinal}", segment.id),
                base_node: format!("{}-base-node-{ordinal}", segment.id),
                object_id: element.object_id,
                version,
                child_object_id,
                source_offset: segment.source_offset + 24,
            });
        }
    }
    Ok(nodes)
}
/// Decode JT 9 geometric-transform attributes from logical scene-graph segments.
pub(super) fn display_jt_geometric_transform_attributes(
    budget: (&DecodeContext<'_>, View<'_>),
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Result<Vec<DisplayJtGeometricTransformAttribute>, CodecError> {
    const GEOMETRIC_TRANSFORM_TYPE: [u8; 16] = [
        0x83, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut attributes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Ok(Vec::new());
        };
        if document.version.major() != 9 || segment.compression.is_none() {
            continue;
        }
        let Some(bytes) = container
            .bounded_entry_bytes(segment.source_offset, u64::from(segment.segment_byte_len))
        else {
            return Ok(Vec::new());
        };
        let Some(compressed) = bytes.get(33..) else {
            return Ok(Vec::new());
        };
        let Some(member) = segment
            .source_offset
            .checked_add(33)
            .and_then(cadmpeg_core::decode::index_from_u64)
            .and_then(|start| {
                start
                    .checked_add(compressed.len())
                    .and_then(|end| budget.1.child(start, end))
            })
        else {
            return Ok(Vec::new());
        };
        let Some(inflated) = inflate_display_jt(budget.0, member)? else {
            return Ok(Vec::new());
        };
        let Some((elements, _)) = parse_jt_element_sequence(budget.0, &inflated)? else {
            return Ok(Vec::new());
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_type_id != GEOMETRIC_TRANSFORM_TYPE {
                continue;
            }
            if element.object_base_type != 3 {
                return Ok(Vec::new());
            }
            let Some((state_flags, field_inhibit_flags, stored_values_mask, matrix)) =
                parse_jt9_geometric_transform_body(element.body)
            else {
                return Ok(Vec::new());
            };
            budget.0.reserve_record_vec(
                &mut attributes,
                1,
                display_jt_text_size(
                    budget.0,
                    &[
                        &segment.id,
                        "-geometric-transform-",
                        &segment.id,
                        "-inflated-element-",
                    ],
                    decimal_digits(ordinal).checked_mul(2).ok_or_else(|| {
                        budget.0.refuse_codec_limit(
                            "store DisplayJT geometric transform",
                            0,
                            u64::MAX,
                        )
                    })?,
                )?,
                "store DisplayJT geometric transform",
            )?;
            attributes.push(DisplayJtGeometricTransformAttribute {
                id: format!("{}-geometric-transform-{ordinal}", segment.id),
                element: format!("{}-inflated-element-{ordinal}", segment.id),
                object_id: element.object_id,
                state_flags,
                field_inhibit_flags,
                stored_values_mask,
                matrix,
                source_offset: segment.source_offset + 24,
            });
        }
    }
    Ok(attributes)
}
/// Decode JT 9 material attributes from logical scene-graph segments.
pub(super) fn display_jt_material_attributes(
    budget: (&DecodeContext<'_>, View<'_>),
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Result<Vec<DisplayJtMaterialAttribute>, CodecError> {
    const MATERIAL_ATTRIBUTE_TYPE: [u8; 16] = [
        0x30, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut attributes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Ok(Vec::new());
        };
        if document.version.major() != 9 || segment.compression.is_none() {
            continue;
        }
        let Some(bytes) = container
            .bounded_entry_bytes(segment.source_offset, u64::from(segment.segment_byte_len))
        else {
            return Ok(Vec::new());
        };
        let Some(compressed) = bytes.get(33..) else {
            return Ok(Vec::new());
        };
        let Some(member) = segment
            .source_offset
            .checked_add(33)
            .and_then(cadmpeg_core::decode::index_from_u64)
            .and_then(|start| {
                start
                    .checked_add(compressed.len())
                    .and_then(|end| budget.1.child(start, end))
            })
        else {
            return Ok(Vec::new());
        };
        let Some(inflated) = inflate_display_jt(budget.0, member)? else {
            return Ok(Vec::new());
        };
        let Some((elements, _)) = parse_jt_element_sequence(budget.0, &inflated)? else {
            return Ok(Vec::new());
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_type_id != MATERIAL_ATTRIBUTE_TYPE {
                continue;
            }
            if element.object_base_type != 3 {
                return Ok(Vec::new());
            }
            let Some((state_flags, field_inhibit_flags, version, data_flags, colors, shininess)) =
                parse_jt9_material_body(element.body)
            else {
                return Ok(Vec::new());
            };
            budget.0.reserve_record_vec(
                &mut attributes,
                1,
                display_jt_text_size(
                    budget.0,
                    &[
                        &segment.id,
                        "-material-attribute-",
                        &segment.id,
                        "-inflated-element-",
                    ],
                    decimal_digits(ordinal).checked_mul(2).ok_or_else(|| {
                        budget.0.refuse_codec_limit(
                            "store DisplayJT material attribute",
                            0,
                            u64::MAX,
                        )
                    })?,
                )?,
                "store DisplayJT material attribute",
            )?;
            attributes.push(DisplayJtMaterialAttribute {
                id: format!("{}-material-attribute-{ordinal}", segment.id),
                element: format!("{}-inflated-element-{ordinal}", segment.id),
                object_id: element.object_id,
                state_flags,
                field_inhibit_flags,
                version,
                data_flags,
                ambient: colors[0],
                diffuse: colors[1],
                specular: colors[2],
                emission: colors[3],
                shininess,

                source_offset: segment.source_offset + 24,
            });
        }
    }
    Ok(attributes)
}
/// Decode complete JT 9 partition nodes from logical scene-graph segments.
pub(super) fn display_jt_partition_nodes(
    budget: (&DecodeContext<'_>, View<'_>),
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Result<Vec<DisplayJtPartitionNode>, CodecError> {
    const PARTITION_NODE_TYPE: [u8; 16] = [
        0x3e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut nodes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Ok(Vec::new());
        };
        if document.version.major() >= 10 {
            continue;
        }
        let Some(bytes) = container
            .bounded_entry_bytes(segment.source_offset, u64::from(segment.segment_byte_len))
        else {
            return Ok(Vec::new());
        };
        let Some(compressed) = bytes.get(33..) else {
            return Ok(Vec::new());
        };
        let Some(member) = segment
            .source_offset
            .checked_add(33)
            .and_then(cadmpeg_core::decode::index_from_u64)
            .and_then(|start| {
                start
                    .checked_add(compressed.len())
                    .and_then(|end| budget.1.child(start, end))
            })
        else {
            return Ok(Vec::new());
        };
        let Some(inflated) = inflate_display_jt(budget.0, member)? else {
            return Ok(Vec::new());
        };
        let Some((elements, _)) = parse_jt_element_sequence(budget.0, &inflated)? else {
            return Ok(Vec::new());
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_type_id != PARTITION_NODE_TYPE {
                continue;
            }
            let ctx = budget.0;
            let Some((_family, _base_reservation)) = admit_jt_group_body(ctx, element.body)? else {
                return Ok(Vec::new());
            };
            let Some(node) = parse_jt9_partition_node_body(ctx, element.body)? else {
                return Ok(Vec::new());
            };
            budget.0.reserve_record_vec(
                &mut nodes,
                1,
                display_jt_text_size(
                    budget.0,
                    &[&segment.id, "-partition-node-", &segment.id, "-base-node-"],
                    decimal_digits(ordinal).checked_mul(2).ok_or_else(|| {
                        budget
                            .0
                            .refuse_codec_limit("store DisplayJT partition node", 0, u64::MAX)
                    })?,
                )?,
                "store DisplayJT partition node",
            )?;
            nodes.push(DisplayJtPartitionNode {
                id: format!("{}-partition-node-{ordinal}", segment.id),
                base_node: format!("{}-base-node-{ordinal}", segment.id),
                object_id: element.object_id,
                group_version: node.group_version,
                child_object_ids: node.child_object_ids,
                file_name: node.file_name,
                transformed_bounds: node.transformed_bounds,
                area: node.area,
                vertex_count_range: node.vertex_count_range,
                node_count_range: node.node_count_range,
                polygon_count_range: node.polygon_count_range,
                bounds: node.bounds,
                source_offset: segment.source_offset + 24,
            });
        }
    }
    Ok(nodes)
}
/// Decode complete JT 9 range-LOD nodes from logical scene-graph segments.
pub(super) fn display_jt_range_lod_nodes(
    budget: (&DecodeContext<'_>, View<'_>),
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Result<Vec<DisplayJtRangeLodNode>, CodecError> {
    const RANGE_LOD_NODE_TYPE: [u8; 16] = [
        0x4c, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut nodes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Ok(Vec::new());
        };
        if document.version.major() >= 10 {
            continue;
        }
        let Some(bytes) = container
            .bounded_entry_bytes(segment.source_offset, u64::from(segment.segment_byte_len))
        else {
            return Ok(Vec::new());
        };
        let Some(compressed) = bytes.get(33..) else {
            return Ok(Vec::new());
        };
        let Some(member) = segment
            .source_offset
            .checked_add(33)
            .and_then(cadmpeg_core::decode::index_from_u64)
            .and_then(|start| {
                start
                    .checked_add(compressed.len())
                    .and_then(|end| budget.1.child(start, end))
            })
        else {
            return Ok(Vec::new());
        };
        let Some(inflated) = inflate_display_jt(budget.0, member)? else {
            return Ok(Vec::new());
        };
        let Some((elements, _)) = parse_jt_element_sequence(budget.0, &inflated)? else {
            return Ok(Vec::new());
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_type_id != RANGE_LOD_NODE_TYPE {
                continue;
            }
            let ctx = budget.0;
            let Some((family, _base_reservation)) = admit_jt_group_body(ctx, element.body)? else {
                return Ok(Vec::new());
            };
            let JtOptionalReservation::Admitted(_vector_reservation) =
                admit_jt_range_vectors(ctx, family)?
            else {
                return Ok(Vec::new());
            };
            let Some(node) = parse_jt9_range_lod_node_body(element.body) else {
                return Ok(Vec::new());
            };
            budget.0.reserve_record_vec(
                &mut nodes,
                1,
                display_jt_text_size(
                    budget.0,
                    &[&segment.id, "-range-lod-node-", &segment.id, "-base-node-"],
                    decimal_digits(ordinal).checked_mul(2).ok_or_else(|| {
                        budget
                            .0
                            .refuse_codec_limit("store DisplayJT range LOD node", 0, u64::MAX)
                    })?,
                )?,
                "store DisplayJT range LOD node",
            )?;
            nodes.push(DisplayJtRangeLodNode {
                id: format!("{}-range-lod-node-{ordinal}", segment.id),
                base_node: format!("{}-base-node-{ordinal}", segment.id),
                object_id: element.object_id,
                group_version: node.group_version,
                child_object_ids: node.child_object_ids,
                lod_version: node.lod_version,
                reserved_values: node.reserved_values,
                reserved_value: node.reserved_value,
                range_version: node.range_version,
                range_limits: node.range_limits,
                center: node.center,
                source_offset: segment.source_offset + 24,
            });
        }
    }
    Ok(nodes)
}
/// Decode complete JT 9 tri-strip shape nodes from logical scene-graph segments.
pub(super) fn display_jt_tri_strip_shape_nodes(
    budget: (&DecodeContext<'_>, View<'_>),
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Result<Vec<DisplayJtTriStripShapeNode>, CodecError> {
    const TRI_STRIP_SHAPE_NODE_TYPE: [u8; 16] = [
        0x77, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut nodes = Vec::new();
    for segment in segments.iter().filter(|segment| segment.segment_type == 1) {
        let Some(document) = documents
            .iter()
            .find(|document| document.id == segment.document)
        else {
            return Ok(Vec::new());
        };
        if document.version.major() != 9 || segment.compression.is_none() {
            continue;
        }
        let Some(bytes) = container
            .bounded_entry_bytes(segment.source_offset, u64::from(segment.segment_byte_len))
        else {
            return Ok(Vec::new());
        };
        let Some(compressed) = bytes.get(33..) else {
            return Ok(Vec::new());
        };
        let Some(member) = segment
            .source_offset
            .checked_add(33)
            .and_then(cadmpeg_core::decode::index_from_u64)
            .and_then(|start| {
                start
                    .checked_add(compressed.len())
                    .and_then(|end| budget.1.child(start, end))
            })
        else {
            return Ok(Vec::new());
        };
        let Some(inflated) = inflate_display_jt(budget.0, member)? else {
            return Ok(Vec::new());
        };
        let Some((elements, _)) = parse_jt_element_sequence(budget.0, &inflated)? else {
            return Ok(Vec::new());
        };
        for (ordinal, element) in elements.into_iter().enumerate() {
            if element.object_type_id != TRI_STRIP_SHAPE_NODE_TYPE {
                continue;
            }
            if element.object_base_type != 2 {
                return Ok(Vec::new());
            }
            let Some((_, _base_reservation)) =
                admit_jt_base_body(budget.0, element.body, 9, false)?
            else {
                return Ok(Vec::new());
            };
            let Some(node) = parse_jt9_tri_strip_shape_node_body(element.body) else {
                return Ok(Vec::new());
            };
            budget.0.reserve_record_vec(
                &mut nodes,
                1,
                display_jt_text_size(
                    budget.0,
                    &[
                        &segment.id,
                        "-tri-strip-shape-node-",
                        &segment.id,
                        "-base-node-",
                    ],
                    decimal_digits(ordinal).checked_mul(2).ok_or_else(|| {
                        budget.0.refuse_codec_limit(
                            "store DisplayJT tri strip shape node",
                            0,
                            u64::MAX,
                        )
                    })?,
                )?,
                "store DisplayJT tri strip shape node",
            )?;
            nodes.push(DisplayJtTriStripShapeNode {
                id: format!("{}-tri-strip-shape-node-{ordinal}", segment.id),
                base_node: format!("{}-base-node-{ordinal}", segment.id),
                object_id: element.object_id,
                reserved_bounds: node.reserved_bounds,
                untransformed_bounds: node.untransformed_bounds,
                area: node.area,
                vertex_count_range: node.vertex_count_range,
                node_count_range: node.node_count_range,
                polygon_count_range: node.polygon_count_range,
                memory_byte_len: node.memory_byte_len,
                compression_level: node.compression_level,
                vertex_version: node.vertex_version,
                vertex_bindings: node.vertex_bindings,
                vertex_quantization_bits: node.vertex_quantization_bits,
                normal_quantization_factor: node.normal_quantization_factor,
                texture_quantization_bits: node.texture_quantization_bits,
                color_quantization_bits: node.color_quantization_bits,

                source_offset: segment.source_offset + 24,
            });
        }
    }
    Ok(nodes)
}
const DISPLAY_JT_COLOR_CHANNEL: u32 = 0x4e58_0001;
const DISPLAY_JT_VERTEX_FLAG_CHANNEL: u32 = 0x4e58_0002;
const DISPLAY_JT_TEXTURE_CHANNEL_BASE: u32 = 0x4e58_0100;
type DisplayJtMatrix = [[f64; 4]; 4];

struct DisplayJtPath {
    matrix: DisplayJtMatrix,
    final_transform: bool,
    diffuse: [Option<UnitBinary32>; 4],
    override_vertex_colors: Option<bool>,
    final_material: bool,
    node_path: Vec<u32>,
    instance_path: Vec<String>,
}

#[derive(Clone, Copy)]
pub(super) struct DisplayJtTessellationInputs<'a> {
    pub(super) meshes: &'a [DisplayJtPolygonMesh],
    pub(super) coordinates: &'a [DisplayJtVertexCoordinates],
    pub(super) normals: &'a [DisplayJtVertexNormals],
    pub(super) colors: &'a [DisplayJtVertexColors],
    pub(super) texture_coordinates: &'a [DisplayJtVertexTextureCoordinates],
    pub(super) vertex_flags: &'a [DisplayJtVertexFlags],
    pub(super) vertex_headers: &'a [DisplayJtCompressedVertexRecordsHeader],
    pub(super) coordinate_headers: &'a [DisplayJtVertexCoordinateArrayHeader],
    pub(super) shape_elements: &'a [DisplayJtShapeLodElement],
    pub(super) bindings: &'a [DisplayJtShapeLodBinding],
    pub(super) shape_nodes: &'a [DisplayJtTriStripShapeNode],
    pub(super) base_nodes: &'a [DisplayJtBaseNodeData],
    pub(super) group_nodes: &'a [DisplayJtGroupNodeData],
    pub(super) instance_nodes: &'a [DisplayJtInstanceNode],
    pub(super) transforms: &'a [DisplayJtGeometricTransformAttribute],
    pub(super) materials: &'a [DisplayJtMaterialAttribute],
    pub(super) compressed_elements: &'a [DisplayJtCompressedElement],
}

fn accumulate_display_jt_material(
    path: &mut DisplayJtPath,
    attribute: &DisplayJtMaterialAttribute,
) {
    const LEGACY_DIFFUSE: u32 = 1 << 1;
    const OVERRIDE_VERTEX_COLORS: u32 = 1 << 5;
    const DIFFUSE_RGB: u32 = 1 << 7;
    const DIFFUSE_ALPHA: u32 = 1 << 8;
    if attribute.state_flags & 0x04 != 0 || path.final_material && attribute.state_flags & 0x02 == 0
    {
        return;
    }
    let rgb_inhibited = match attribute.version {
        JtMaterialVersion::One => attribute.field_inhibit_flags & LEGACY_DIFFUSE != 0,
        JtMaterialVersion::Two(_) => attribute.field_inhibit_flags & DIFFUSE_RGB != 0,
    };
    let alpha_inhibited = match attribute.version {
        JtMaterialVersion::One => attribute.field_inhibit_flags & LEGACY_DIFFUSE != 0,
        JtMaterialVersion::Two(_) => attribute.field_inhibit_flags & DIFFUSE_ALPHA != 0,
    };
    if !rgb_inhibited {
        for (target, component) in path.diffuse[..3].iter_mut().zip(attribute.diffuse) {
            *target = Some(component);
        }
    }
    if !alpha_inhibited {
        path.diffuse[3] = Some(attribute.diffuse[3]);
    }
    if attribute.field_inhibit_flags & OVERRIDE_VERTEX_COLORS == 0 {
        path.override_vertex_colors = Some(attribute.data_flags & 0x20 != 0);
    }
    path.final_material |= attribute.state_flags & 0x01 != 0;
}

fn display_jt_path_color(path: &DisplayJtPath) -> Option<Color> {
    Some(Color::from_unit_binary32([
        path.diffuse[0]?,
        path.diffuse[1]?,
        path.diffuse[2]?,
        path.diffuse[3]?,
    ]))
}

fn multiply_jt_matrices(left: DisplayJtMatrix, right: DisplayJtMatrix) -> Option<DisplayJtMatrix> {
    let mut product = [[0.0; 4]; 4];
    for (row, values) in product.iter_mut().enumerate() {
        for (column, value) in values.iter_mut().enumerate() {
            *value = (0..4)
                .map(|inner| left[row][inner] * right[inner][column])
                .sum();
            if !value.is_finite() {
                return None;
            }
        }
    }
    Some(product)
}

struct JtPathLookup<'a, 'b> {
    by_object: &'a BTreeMap<u32, &'b DisplayJtBaseNodeData>,
    parents: &'a BTreeMap<u32, Vec<u32>>,
    instance_ids: &'a BTreeMap<u32, String>,
    transforms: &'a [&'b DisplayJtGeometricTransformAttribute],
    materials: &'a [&'b DisplayJtMaterialAttribute],
}

fn resolve_display_jt_node_paths(
    ctx: &DecodeContext<'_>,
    object_id: u32,
    lookup: &JtPathLookup<'_, '_>,
    visiting: &mut BTreeSet<u32>,
    visiting_reservation: &mut ScopedReservation<'_>,
) -> Result<Option<Vec<DisplayJtPath>>, CodecError> {
    let _depth = ctx.enter_nested("resolve JT node path")?;
    let Some(base) = lookup.by_object.get(&object_id) else {
        return Ok(None);
    };
    if base.flags & 1 != 0 {
        return Ok(Some(Vec::new()));
    }
    if visiting.contains(&object_id) {
        return Ok(None);
    }
    visiting_reservation.grow(cadmpeg_core::decode::u64_from_index(
        std::mem::size_of::<u32>() * 4,
    ))?;
    ctx.insert_btree_set(visiting, object_id, "nx JT visiting nodes")?;
    let mut parent_states = Vec::new();
    let mut parent_states_reservation = ctx.reserve_scoped(0, "nx JT parent path states")?;
    if let Some(ids) = lookup.parents.get(&object_id) {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(ids.len()),
            "resolve JT parent paths",
        )?;
        for id in ids {
            let Some(paths) =
                resolve_display_jt_node_paths(ctx, *id, lookup, visiting, visiting_reservation)?
            else {
                return Ok(None);
            };
            let count = paths.len();
            ctx.reserve_scoped_vec(
                &mut parent_states_reservation,
                &mut parent_states,
                count,
                "nx JT parent path states",
            )?;
            parent_states.extend(paths);
        }
    } else {
        ctx.reserve_scoped_vec(
            &mut parent_states_reservation,
            &mut parent_states,
            1,
            "nx JT root path state",
        )?;
        parent_states.push(DisplayJtPath {
            matrix: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            final_transform: false,
            diffuse: [None; 4],
            override_vertex_colors: None,
            final_material: false,
            node_path: Vec::new(),
            instance_path: Vec::new(),
        });
    }
    visiting.remove(&object_id);
    let mut results = Vec::new();
    for mut path in parent_states {
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(base.attribute_object_ids.len()),
            "resolve JT path attributes",
        )?;
        for attribute_id in &base.attribute_object_ids {
            let mut matching_transforms = lookup
                .transforms
                .iter()
                .filter(|attribute| attribute.object_id == *attribute_id)
                .copied();
            let transform = matching_transforms.next();
            let duplicate_transform = matching_transforms.next().is_some();
            let mut matching_materials = lookup
                .materials
                .iter()
                .filter(|attribute| attribute.object_id == *attribute_id)
                .copied();
            let material = matching_materials.next();
            if duplicate_transform
                || matching_materials.next().is_some()
                || transform.is_some() && material.is_some()
            {
                return Ok(None);
            }
            if let Some(attribute) = transform {
                if attribute.state_flags & 0x04 == 0
                    && (!path.final_transform || attribute.state_flags & 0x02 != 0)
                {
                    let local = attribute.matrix.get().map(|row| row.map(f64::from));
                    let Some(matrix) = multiply_jt_matrices(local, path.matrix) else {
                        return Ok(None);
                    };
                    path.matrix = matrix;
                    path.final_transform |= attribute.state_flags & 0x01 != 0;
                }
            }
            if let Some(attribute) = material {
                accumulate_display_jt_material(&mut path, attribute);
            }
        }
        if let Some(instance_id) = lookup.instance_ids.get(&object_id) {
            ctx.reserve_vec(&mut path.instance_path, 1, "nx JT instance path nodes")?;
            path.instance_path.push(ctx.join_retained(
                &[instance_id],
                "",
                "nx JT instance path identity",
            )?);
        }
        ctx.reserve_vec(&mut path.node_path, 1, "nx JT node path nodes")?;
        path.node_path.push(object_id);
        ctx.reserve_retained_vec(&mut results, 1, "nx JT resolved paths")?;
        results.push(path);
    }
    Ok(Some(results))
}

fn display_jt_node_paths(
    ctx: &DecodeContext<'_>,
    scene_segment: &str,
    shape_object_id: u32,
    inputs: &DisplayJtTessellationInputs<'_>,
) -> Result<Option<Vec<DisplayJtPath>>, CodecError> {
    let scene_scans = inputs
        .base_nodes
        .len()
        .checked_add(inputs.transforms.len())
        .and_then(|count| count.checked_add(inputs.materials.len()))
        .and_then(|count| count.checked_mul(2))
        .and_then(|count| count.checked_mul(inputs.compressed_elements.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("scan JT node paths", 0, 1))?;
    let instance_scans = inputs
        .instance_nodes
        .len()
        .checked_mul(inputs.base_nodes.len())
        .ok_or_else(|| ctx.refuse_codec_limit("scan JT node paths", 0, 1))?;
    let child_scans = inputs
        .group_nodes
        .len()
        .checked_add(inputs.instance_nodes.len())
        .and_then(|count| count.checked_mul(inputs.base_nodes.len()))
        .ok_or_else(|| ctx.refuse_codec_limit("scan JT node paths", 0, 1))?;
    let work = scene_scans
        .checked_add(instance_scans)
        .and_then(|count| count.checked_add(child_scans))
        .ok_or_else(|| ctx.refuse_codec_limit("scan JT node paths", 0, 1))?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(work),
        "scan JT node paths",
    )?;
    let in_scene = |element_id: &str| {
        inputs
            .compressed_elements
            .iter()
            .find(|element| element.id == element_id)
            .is_some_and(|element| element.segment == scene_segment)
    };
    let scoped_count = inputs
        .base_nodes
        .iter()
        .filter(|base| in_scene(&base.element))
        .count();
    let mut scoped = Vec::new();
    let _scoped_reservation =
        ctx.reserve_temporary_vec(&mut scoped, scoped_count, "nx JT scoped base nodes")?;
    scoped.extend(
        inputs
            .base_nodes
            .iter()
            .filter(|base| in_scene(&base.element)),
    );
    let mut by_object = BTreeMap::new();
    let mut by_object_reservation = ctx.reserve_scoped(0, "nx JT node index")?;
    for base in &scoped {
        if by_object.contains_key(&base.object_id) {
            return Ok(None);
        }
        by_object_reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<(u32, &DisplayJtBaseNodeData)>() * 4,
        ))?;
        ctx.insert_btree_map(&mut by_object, base.object_id, *base, "nx JT node index")?;
    }
    if !by_object.contains_key(&shape_object_id) {
        return Ok(None);
    }
    let transform_count = inputs
        .transforms
        .iter()
        .filter(|attribute| in_scene(&attribute.element))
        .count();
    let mut scoped_transforms = Vec::new();
    let _transform_reservation = ctx.reserve_temporary_vec(
        &mut scoped_transforms,
        transform_count,
        "nx JT scoped transforms",
    )?;
    scoped_transforms.extend(
        inputs
            .transforms
            .iter()
            .filter(|attribute| in_scene(&attribute.element)),
    );
    let material_count = inputs
        .materials
        .iter()
        .filter(|attribute| in_scene(&attribute.element))
        .count();
    let mut scoped_materials = Vec::new();
    let _material_reservation = ctx.reserve_temporary_vec(
        &mut scoped_materials,
        material_count,
        "nx JT scoped materials",
    )?;
    scoped_materials.extend(
        inputs
            .materials
            .iter()
            .filter(|attribute| in_scene(&attribute.element)),
    );
    let mut parents = BTreeMap::<u32, Vec<u32>>::new();
    let mut parents_reservation = ctx.reserve_scoped(0, "nx JT parent index")?;
    let mut instance_ids = BTreeMap::new();
    let mut instances_reservation = ctx.reserve_scoped(0, "nx JT instance index")?;
    for node in inputs.instance_nodes {
        if !by_object.values().any(|base| base.id == node.base_node) {
            continue;
        }
        if instance_ids.contains_key(&node.object_id) {
            return Ok(None);
        }
        instances_reservation.grow(cadmpeg_core::decode::u64_from_index(
            std::mem::size_of::<(u32, String)>() * 4,
        ))?;
        let id = ctx.join_retained(&[&node.id], "", "nx JT instance identity")?;
        ctx.insert_btree_map(
            &mut instance_ids,
            node.object_id,
            id,
            "nx JT instance index",
        )?;
    }
    for (object_id, base) in &by_object {
        let mut group_children = inputs
            .group_nodes
            .iter()
            .filter(|node| node.base_node == base.id)
            .map(|node| node.child_object_ids.as_slice());
        let group = group_children.next();
        if group_children.next().is_some() {
            return Ok(None);
        }
        let mut instance_children = inputs
            .instance_nodes
            .iter()
            .filter(|node| node.base_node == base.id)
            .map(|node| std::slice::from_ref(&node.child_object_id));
        let instance = instance_children.next();
        if instance_children.next().is_some() || group.is_some() && instance.is_some() {
            return Ok(None);
        }
        let children = group.or(instance).unwrap_or_default();
        for &child in children {
            if !by_object.contains_key(&child) {
                return Ok(None);
            }
            if !parents.contains_key(&child) {
                ctx.admit_btree_entry(&parents, &child, "nx JT parent index")?;
                parents_reservation.grow(cadmpeg_core::decode::u64_from_index(
                    std::mem::size_of::<(u32, Vec<u32>)>() * 4,
                ))?;
            }
            let ids = parents.entry(child).or_default();
            ctx.reserve_scoped_vec(&mut parents_reservation, ids, 1, "nx JT parent references")?;
            ids.push(*object_id);
        }
    }
    let lookup = JtPathLookup {
        by_object: &by_object,
        parents: &parents,
        instance_ids: &instance_ids,
        transforms: &scoped_transforms,
        materials: &scoped_materials,
    };
    let mut visiting_reservation = ctx.reserve_scoped(0, "nx JT visiting nodes")?;
    resolve_display_jt_node_paths(
        ctx,
        shape_object_id,
        &lookup,
        &mut BTreeSet::new(),
        &mut visiting_reservation,
    )
}

fn transform_jt_point(matrix: [[f64; 4]; 4], point: [f32; 3]) -> Option<FinitePoint3> {
    let point = point.map(f64::from);
    let coordinate = |column| {
        (matrix[3][column]
            + (0..3)
                .map(|row| point[row] * matrix[row][column])
                .sum::<f64>())
            * 1000.0
    };
    let point = Point3::new(coordinate(0), coordinate(1), coordinate(2));
    FinitePoint3::new(point)
}

fn transform_jt_normal(matrix: [[f64; 4]; 4], normal: [f32; 3]) -> Option<UnitVector3> {
    let a = matrix;
    let determinant = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    if !determinant.is_finite() || determinant == 0.0 {
        return None;
    }
    let inverse = [
        [
            (a[1][1] * a[2][2] - a[1][2] * a[2][1]) / determinant,
            (a[0][2] * a[2][1] - a[0][1] * a[2][2]) / determinant,
            (a[0][1] * a[1][2] - a[0][2] * a[1][1]) / determinant,
        ],
        [
            (a[1][2] * a[2][0] - a[1][0] * a[2][2]) / determinant,
            (a[0][0] * a[2][2] - a[0][2] * a[2][0]) / determinant,
            (a[0][2] * a[1][0] - a[0][0] * a[1][2]) / determinant,
        ],
        [
            (a[1][0] * a[2][1] - a[1][1] * a[2][0]) / determinant,
            (a[0][1] * a[2][0] - a[0][0] * a[2][1]) / determinant,
            (a[0][0] * a[1][1] - a[0][1] * a[1][0]) / determinant,
        ],
    ];
    let normal = normal.map(f64::from);
    let transformed = Vector3::new(
        (0..3).map(|index| normal[index] * inverse[0][index]).sum(),
        (0..3).map(|index| normal[index] * inverse[1][index]).sum(),
        (0..3).map(|index| normal[index] * inverse[2][index]).sum(),
    );
    UnitVector3::normalized_with_length(transformed).map(|(direction, _)| direction)
}

/// Every Display-JT tessellation the shape graph states.
///
/// A record the graph does not shape as a mesh yields no tessellation and is
/// not an error; a mesh whose lanes do not pair is.
///
/// # Errors
///
/// Refuses a vertex-normal lane that does not cover the vertex lane, and a
/// tessellation the IR will not admit.
pub(super) fn display_jt_tessellations(
    ctx: &DecodeContext<'_>,
    inputs: &DisplayJtTessellationInputs<'_>,
) -> Result<Vec<(Tessellation, u64)>, CodecError> {
    let rows = display_jt_tessellation_rows(ctx, inputs)?;
    ctx.charge_work(0, "complete JT tessellation decode")?;
    Ok(rows.unwrap_or_default())
}

fn display_jt_tessellation_rows(
    ctx: &DecodeContext<'_>,
    inputs: &DisplayJtTessellationInputs<'_>,
) -> Result<Option<Vec<(Tessellation, u64)>>, CodecError> {
    macro_rules! required {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let DisplayJtTessellationInputs {
        meshes,
        coordinates,
        normals,
        colors,
        texture_coordinates,
        vertex_flags,
        vertex_headers,
        coordinate_headers,
        shape_elements,
        bindings,
        shape_nodes,
        base_nodes,
        compressed_elements,
        ..
    } = *inputs;
    let mut tessellations = Vec::new();
    for mesh in meshes {
        let coordinate_header = required!(coordinate_headers
            .iter()
            .find(|header| header.id == mesh.coordinate_header));
        let coordinates = required!(coordinates
            .iter()
            .find(|coordinates| coordinates.header == coordinate_header.id));
        let shape_element = required!(shape_elements
            .iter()
            .find(|element| element.id == coordinate_header.element));
        let mut matching_bindings = bindings.iter().filter(|binding| {
            binding.shape_segment == shape_element.segment
                && binding.payload_object_id == shape_element.object_id
        });
        let binding = required!(matching_bindings.next());
        if matching_bindings.next().is_some() {
            return Ok(None);
        }
        let mut matching_nodes = shape_nodes.iter().filter(|node| {
            if node.object_id != binding.shape_node_object_id {
                return false;
            }
            let Some(base) = base_nodes.iter().find(|base| base.id == node.base_node) else {
                return false;
            };
            compressed_elements
                .iter()
                .find(|element| element.id == base.element)
                .is_some_and(|element| element.segment == binding.scene_segment)
        });
        let shape_node = required!(matching_nodes.next());
        if matching_nodes.next().is_some() {
            return Ok(None);
        }
        let paths = required!(display_jt_node_paths(
            ctx,
            &binding.scene_segment,
            shape_node.object_id,
            inputs
        )?);
        let mut rendered = Vec::new();
        let render_count = mesh
            .polygons
            .iter()
            .filter(|polygon| polygon.group >= 0)
            .count();
        let _render_reservation =
            (ctx.reserve_temporary_vec(&mut rendered, render_count, "nx JT rendered triangles"))?;
        for polygon in &mesh.polygons {
            if polygon.group < 0 {
                continue;
            }
            let corners: &[(u32, Option<u32>); 3] =
                required!(polygon.corners.as_slice().try_into().ok());
            let triangle = corners.map(|(vertex, _)| vertex);
            let attributes = corners.map(|(_, attribute)| attribute);
            rendered.push((triangle, attributes));
        }
        if rendered.is_empty() {
            return Ok(None);
        }
        let vertex_header = required!(vertex_headers
            .iter()
            .find(|header| header.element == shape_element.id));
        let normal_array = (vertex_header.vertex_bindings & 0x8 != 0)
            .then(|| {
                normals
                    .iter()
                    .find(|normals| normals.vertex_records_header == vertex_header.id)
            })
            .flatten();
        if vertex_header.vertex_bindings & 0x8 != 0 && normal_array.is_none() {
            return Ok(None);
        }
        let color_array = (vertex_header.vertex_bindings & 0x30 != 0)
            .then(|| {
                colors
                    .iter()
                    .find(|colors| colors.vertex_records_header == vertex_header.id)
            })
            .flatten();
        if vertex_header.vertex_bindings & 0x30 != 0 && color_array.is_none() {
            return Ok(None);
        }
        let texture_channels = (0..8_u8)
            .filter(|channel| vertex_header.vertex_bindings & (0xf_u64 << (8 + 4 * channel)) != 0)
            .count();
        let mut texture_arrays = Vec::new();
        let _texture_array_reservation = (ctx.reserve_temporary_vec(
            &mut texture_arrays,
            texture_channels,
            "nx JT texture array references",
        ))?;
        for channel in (0..8_u8)
            .filter(|channel| vertex_header.vertex_bindings & (0xf_u64 << (8 + 4 * channel)) != 0)
        {
            let array = required!(texture_coordinates.iter().find(|coordinates| {
                coordinates.vertex_records_header == vertex_header.id
                    && coordinates.channel == channel
            }));
            texture_arrays.push(array);
        }
        let vertex_flag_array = (vertex_header.vertex_bindings & 0x40 != 0)
            .then(|| {
                vertex_flags
                    .iter()
                    .find(|flags| flags.vertex_records_header == vertex_header.id)
            })
            .flatten();
        if vertex_header.vertex_bindings & 0x40 != 0 && vertex_flag_array.is_none() {
            return Ok(None);
        }
        for path in paths {
            let transform = path.matrix;
            let color = if color_array.is_none() || path.override_vertex_colors == Some(true) {
                display_jt_path_color(&path)
            } else {
                None
            };
            let instance_path = path.instance_path;
            let node_path =
                ctx.join_display_retained(path.node_path.iter(), "-", "nx JT rendered node path")?;
            let convert_point = |index: u32| {
                let point = coordinates.points_m.get(usize::try_from(index).ok()?)?;
                transform_jt_point(transform, point.map(FiniteBinary32::get))
            };
            let has_vertex_attributes = normal_array.is_some()
                || color_array.is_some()
                || !texture_arrays.is_empty()
                || vertex_flag_array.is_some();
            let (vertices, triangles, normal_vectors, channels) = if has_vertex_attributes {
                let triangle_vertex_count = required!(rendered.len().checked_mul(3));
                let mut vertices = Vec::new();
                (ctx.reserve_retained_vec(
                    &mut vertices,
                    triangle_vertex_count,
                    "nx JT tessellation vertices",
                ))?;
                let mut triangles = Vec::new();
                (ctx.reserve_retained_vec(
                    &mut triangles,
                    rendered.len(),
                    "nx JT tessellation triangles",
                ))?;
                // An unshaded mesh is stated by absence: no normal record, no
                // normal lane.
                let mut normal_vectors = normal_array.is_some().then(Vec::new);
                if let Some(normal_vectors) = normal_vectors.as_mut() {
                    (ctx.reserve_retained_vec(
                        normal_vectors,
                        triangle_vertex_count,
                        "nx JT tessellation normals",
                    ))?;
                }
                let mut color_data = Vec::new();
                if color_array.is_some() {
                    let color_byte_count = required!(triangle_vertex_count.checked_mul(16));
                    (ctx.reserve_retained_vec(
                        &mut color_data,
                        color_byte_count,
                        "nx JT tessellation colors",
                    ))?;
                }
                let mut texture_component_counts = Vec::new();
                let _texture_count_reservation = (ctx.reserve_temporary_vec(
                    &mut texture_component_counts,
                    texture_arrays.len(),
                    "nx JT texture component counts",
                ))?;
                for array in &texture_arrays {
                    let count = required!(array.values.first()).len();
                    if !(1..=4).contains(&count)
                        || !array.values.iter().all(|value| value.len() == count)
                    {
                        return Ok(None);
                    }
                    texture_component_counts.push(count);
                }
                let mut texture_data = Vec::new();
                (ctx.reserve_retained_vec(
                    &mut texture_data,
                    texture_component_counts.len(),
                    "nx JT tessellation texture buffers",
                ))?;
                for component_count in &texture_component_counts {
                    let byte_count = required!(triangle_vertex_count
                        .checked_mul(*component_count)
                        .and_then(|count| count.checked_mul(4)));
                    let mut data = Vec::new();
                    (ctx.reserve_retained_vec(
                        &mut data,
                        byte_count,
                        "nx JT tessellation texture bytes",
                    ))?;
                    texture_data.push(data);
                }
                let mut vertex_flag_data = Vec::new();
                if vertex_flag_array.is_some() {
                    let flag_byte_count = required!(triangle_vertex_count.checked_mul(4));
                    (ctx.reserve_retained_vec(
                        &mut vertex_flag_data,
                        flag_byte_count,
                        "nx JT tessellation flag bytes",
                    ))?;
                }
                for (triangle, attributes) in rendered.iter().copied() {
                    let base = required!(u32::try_from(vertices.len()).ok());
                    for (coordinate, attribute) in triangle.into_iter().zip(attributes) {
                        vertices.push(required!(convert_point(coordinate)));
                        let attribute = required!(usize::try_from(required!(attribute)).ok());
                        if let (Some(normal_array), Some(normal_vectors)) =
                            (normal_array, normal_vectors.as_mut())
                        {
                            let normal = required!(normal_array.normals.get(attribute));
                            normal_vectors.push(FiniteVector3::from(required!(
                                transform_jt_normal(transform, normal.map(FiniteBinary32::get),)
                            )));
                        }
                        if let Some(color_array) = color_array {
                            for component in required!(color_array.colors.get(attribute)) {
                                color_data.extend_from_slice(&component.get().to_le_bytes());
                            }
                        }
                        for (array, data) in texture_arrays.iter().zip(&mut texture_data) {
                            for component in required!(array.values.get(attribute)) {
                                data.extend_from_slice(&component.get().to_le_bytes());
                            }
                        }
                        if let Some(array) = vertex_flag_array {
                            vertex_flag_data.extend_from_slice(
                                &required!(array.values.get(attribute)).to_le_bytes(),
                            );
                        }
                    }
                    triangles.push([
                        base,
                        required!(base.checked_add(1)),
                        required!(base.checked_add(2)),
                    ]);
                }
                let mut channels = Vec::new();
                let channel_count = required!(usize::from(color_array.is_some())
                    .checked_add(texture_arrays.len())
                    .and_then(|count| count.checked_add(usize::from(vertex_flag_array.is_some()))));
                (ctx.reserve_retained_vec(
                    &mut channels,
                    channel_count,
                    "nx JT tessellation channels",
                ))?;
                if color_array.is_some() {
                    channels.push(required!(TessellationChannel::new(
                        cadmpeg_ir::tessellation::ChannelAddressing::Vertex {},
                        16,
                        DISPLAY_JT_COLOR_CHANNEL,
                        required!(u32::try_from((vertex_header.vertex_bindings >> 4) & 0x3).ok()),
                        color_data,
                    )
                    .ok()));
                }
                for (((array, component_count), data), ordinal) in texture_arrays
                    .iter()
                    .zip(texture_component_counts)
                    .zip(texture_data)
                    .zip(0_u32..)
                {
                    channels.push(required!(TessellationChannel::new(
                        cadmpeg_ir::tessellation::ChannelAddressing::Vertex {},
                        required!(u32::try_from(required!(component_count.checked_mul(4))).ok()),
                        required!(DISPLAY_JT_TEXTURE_CHANNEL_BASE.checked_add(ordinal)),
                        u32::from(array.channel)
                            | required!(u32::try_from(
                                (vertex_header.vertex_bindings >> (8 + 4 * array.channel)) & 0xf
                            )
                            .ok())
                                << 8,
                        data,
                    )
                    .ok()));
                }
                if vertex_flag_array.is_some() {
                    channels.push(required!(TessellationChannel::new(
                        cadmpeg_ir::tessellation::ChannelAddressing::Vertex {},
                        4,
                        DISPLAY_JT_VERTEX_FLAG_CHANNEL,
                        0,
                        vertex_flag_data,
                    )
                    .ok()));
                }
                (vertices, triangles, normal_vectors, channels)
            } else {
                let mut vertices = Vec::new();
                (ctx.reserve_retained_vec(
                    &mut vertices,
                    coordinates.points_m.len(),
                    "nx JT tessellation vertices",
                ))?;
                for index in 0..coordinates.points_m.len() {
                    vertices.push(required!(convert_point(required!(
                        u32::try_from(index).ok()
                    ))));
                }
                let mut triangles = Vec::new();
                (ctx.reserve_retained_vec(
                    &mut triangles,
                    rendered.len(),
                    "nx JT tessellation triangles",
                ))?;
                triangles.extend(rendered.iter().map(|(triangle, _)| *triangle));
                (vertices, triangles, None, Vec::new())
            };
            (ctx.reserve_record_vec(&mut tessellations, 1, 0, "nx JT tessellations"))?;
            let tessellation_id = (retain_jt_tessellation_id(
                ctx,
                shape_element.source_offset,
                shape_element.object_id,
                (path.node_path.len() != 1).then_some(node_path.as_str()),
            ))?;
            let mesh = match cadmpeg_ir::tessellation::TessellationMesh::from_checked_list_lanes(
                vertices,
                triangles,
                normal_vectors,
            ) {
                Ok(mesh) => mesh,
                Err(error) => {
                    return Err(CodecError::malformed(format_args!(
                        "display-jt tessellation: {error}"
                    )));
                }
            };
            let tessellation = (Tessellation::from_parts(tessellation_id, mesh, channels)
                .map_err(|error| {
                    CodecError::malformed(format_args!("display-jt tessellation: {error}"))
                }))?;
            tessellations.push((
                tessellation.with_source_object(Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Nx,
                    object_id: required!(cadmpeg_core::text::NonBlankString::new(
                        (ctx.join_retained(
                            &[&shape_node.id],
                            "",
                            "nx JT tessellation source identity"
                        ))?,
                    )),
                    name: None,
                    color,
                    visible: None,
                    layer: None,
                    instance_path,
                })),
                shape_node.source_offset,
            ));
        }
    }
    Ok(Some(tessellations))
}

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_reflectivity, f32, "reflectivity");
