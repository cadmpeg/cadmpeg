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

use std::collections::{BTreeMap, HashMap};

use serde::ser::SerializeSeq;
use serde::{Deserialize, Serialize};

use crate::container::Container;
use cadmpeg_core::CodecError;

use cadmpeg_ir::hash::digest::Sha256Digest;
use packet_role::{TopologyContext, TopologyPacketRole};
use version::JtVersionField;

use cadmpeg_container::compression::inflate_zlib_member_owned;
use cadmpeg_core::bytes::{assemble_f32_le, assemble_u32_le, assemble_u64_le};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteBinary32, UnitBinary32};
use cadmpeg_ir::tessellation::{Tessellation, TessellationChannel};
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::{topology::Color, SourceObjectAssociation};

use std::convert::Infallible;
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
    let Some(index) = ctx.find_text(source, from, operation)? else {
        return ctx.join_retained(&[source], "", operation);
    };
    let after = index + from.len();
    ctx.join_retained(&[&source[..index], to, &source[after..]], "", operation)
}

/// A rejected identity whose input drops before its scoped allocation receipt.
struct JtTessellationIdentityRefusal<'ctx> {
    error: cadmpeg_ir::ids::IdentityError,
    _storage: ScopedReservation<'ctx>,
}

fn retain_jt_tessellation_id<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    offset: u64,
    object_id: u32,
    path: Option<&str>,
) -> Result<Result<cadmpeg_ir::tessellation::TessellationId, JtTessellationIdentityRefusal<'ctx>>, CodecError> {
    let operation = "nx JT tessellation identity";
    let mut id_storage = ctx.reserve_scoped(0, operation)?;
    let id = id_storage.with_storage(|| match path {
        Some(path) => ctx.format_retained(
            format_args!("nx:display-jt:tessellation#{offset}-{object_id}-path-{path}"),
            operation,
        ),
        None => ctx.format_retained(
            format_args!("nx:display-jt:tessellation#{offset}-{object_id}"),
            operation,
        ),
    })?;
    match cadmpeg_ir::tessellation::TessellationId::mint(id) {
        Ok(id) => {
            id_storage.commit()?;
            Ok(Ok(id))
        }
        Err(error) => Ok(Err(JtTessellationIdentityRefusal { error, _storage: id_storage })),
    }
}

fn inflate_display_jt(
    ctx: &DecodeContext<'_>,
    member: View<'_>,
) -> Result<Option<Vec<u8>>, CodecError> {
    let (buffer, consumed) =
        match inflate_zlib_member_owned(ctx, member, cadmpeg_core::decode::ExpandSpec::Unknown) {
            Ok(output) => output,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => return Ok(None),
        };
    if consumed != member.window().len() {
        return Ok(None);
    }
    ctx.charge_retained(
        cadmpeg_core::decode::u64_from_index(buffer.capacity()),
        "retain inflated DisplayJT payload",
    )?;
    Ok(Some(buffer))
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
        let rows = NonEmpty::from_admitted_vec(rows).ok_or("rows: at least one row is required")?;
        Ok(Self {
            id,
            version,
            rows,
            source_offset,
        })
    }

    /// Number of indexed JT documents.
    pub(super) fn declared_count(&self) -> usize {
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
        let version = JtVersionField::new(&wire.version_field)?;
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

/// Traversal admission for the range-limit check that serde and decode share.
trait JtRangeLimitAdmission {
    type Error;
    type Iter<'a>: Iterator<Item = &'a FiniteBinary32>
    where
        Self: 'a;

    fn admit<'a>(&'a self, values: &'a [FiniteBinary32]) -> Result<Self::Iter<'a>, Self::Error>;
}

struct ContextFreeJtRangeLimitAdmission;

impl JtRangeLimitAdmission for ContextFreeJtRangeLimitAdmission {
    type Error = Infallible;
    type Iter<'a>
        = std::slice::Iter<'a, FiniteBinary32>
    where
        Self: 'a;

    fn admit<'a>(&'a self, values: &'a [FiniteBinary32]) -> Result<Self::Iter<'a>, Self::Error> {
        Ok(values.iter())
    }
}

struct DecodeJtRangeLimitAdmission<'ctx, 'decode> {
    ctx: &'ctx DecodeContext<'decode>,
}

impl JtRangeLimitAdmission for DecodeJtRangeLimitAdmission<'_, '_> {
    type Error = CodecError;
    type Iter<'a>
        = cadmpeg_core::decode::scan::AdmittedIter<std::slice::Iter<'a, FiniteBinary32>>
    where
        Self: 'a;

    fn admit<'a>(&'a self, values: &'a [FiniteBinary32]) -> Result<Self::Iter<'a>, Self::Error> {
        Ok(self
            .ctx
            .admit_iter(values, "validate DisplayJT range limits")?)
    }
}

impl JtRangeLimits {
    /// Accepts strictly increasing distances whose first value is nonnegative,
    /// so every later value is nonnegative too.
    fn from_finite<A: JtRangeLimitAdmission>(
        values: Vec<FiniteBinary32>,
        admission: &A,
    ) -> Result<Result<Self, &'static str>, A::Error> {
        let mut previous = None;
        for value in admission.admit(&values)? {
            let value = value.get();
            if previous.map_or(value < 0.0, |previous| previous >= value) {
                return Ok(Err(
                    "range_limits: expected finite nonnegative strictly increasing distances",
                ));
            }
            previous = Some(value);
        }
        Ok(Ok(Self(values)))
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
        match Self::from_finite(values, &ContextFreeJtRangeLimitAdmission) {
            Ok(limits) => limits,
            Err(error) => match error {},
        }
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
        let rows = [0, 1, 2].map(|row| [0, 1, 2].map(|column| f64::from(raw[row][column])));
        let lengths = rows.map(|row| {
            row.iter()
                .fold(0.0_f64, |length, value| length.hypot(*value))
        });
        if lengths
            .iter()
            .any(|length| !length.is_finite() || *length == 0.0)
        {
            return None;
        }
        for (first, second) in [(0, 1), (0, 2), (1, 2)] {
            let dot = rows[first]
                .iter()
                .zip(rows[second])
                .map(|(left, right)| left * right)
                .sum::<f64>();
            if dot.abs() > 1.0e-5 * lengths[first] * lengths[second] {
                return None;
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
        match Self::from_wire(wire, |tail| Ok::<_, Infallible>(Sha256Digest::digest(tail))) {
            Ok(sequence) => sequence,
            Err(error) => match error {},
        }
    }
}

impl DisplayJtCompressedElementSequence {
    /// Checks a serialized sequence, hashing its tail through `digest`.
    fn from_wire<E>(
        wire: DisplayJtCompressedElementSequenceWire,
        digest: impl FnOnce(&[u8]) -> Result<Sha256Digest, E>,
    ) -> Result<Result<Self, &'static str>, E> {
        if wire.tail_sha256 != digest(&wire.tail)? {
            return Ok(Err(
                "DisplayJtCompressedElementSequence.tail_sha256 disagrees with tail",
            ));
        }
        Ok(Self::with_tail_digest(wire))
    }

    /// Checks the framed extent of a sequence whose tail digest is already
    /// known to belong to its tail.
    fn with_tail_digest(
        wire: DisplayJtCompressedElementSequenceWire,
    ) -> Result<Self, &'static str> {
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
        ctx.reserve_vec(&mut elements, 1, "store DisplayJT element")?;
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
        ctx.charge_work(1, "scan JT topology high-degree lanes")?;
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

fn parse_jt_base_node_body(body: &[u8], format_major: u16) -> Option<(u16, u32, &[u8], &[u8])> {
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
    let count = view.counted(u64::from(attribute_count), 4)?.get();
    let attribute_object_ids = view.take(count.checked_mul(4)?)?;
    Some((
        version,
        flags,
        attribute_object_ids,
        body.get(view.position()..)?,
    ))
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

fn read_jt_object_ids(
    ctx: &DecodeContext<'_>,
    encoded: &[u8],
    operation: &'static str,
) -> Result<Vec<u32>, CodecError> {
    ctx.try_collect_retained_with(
        encoded.chunks_exact(4),
        operation,
        |bytes| -> Result<u32, CodecError> { Ok(View::over_retained(bytes).req_u32_le()?) },
    )
}

fn parse_jt9_group_data(bytes: &[u8]) -> Option<(u16, &[u8], &[u8])> {
    let mut view = View::over_retained(bytes);
    let version = view.u16_le()?;
    let count = view.u32_le()?;
    let count = view.counted(u64::from(count), 4)?.get();
    let children = view.take(count.checked_mul(4)?)?;
    Some((version, children, bytes.get(view.position()..)?))
}

fn parse_jt9_group_node_body(body: &[u8]) -> Option<(u16, &[u8], &[u8])> {
    let (_, _, _, family) = parse_jt_base_node_body(body, 9)?;
    parse_jt9_group_data(family)
}

fn parse_jt9_partition_node_body(
    ctx: &DecodeContext<'_>,
    body: &[u8],
) -> Result<Option<ParsedJtPartitionNode>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "DisplayJT partition candidate")?;
    let parsed = storage.with_storage(|| {
    let parsed = (|| {
        let (_, _, _, family) = parse_jt_base_node_body(body, 9)?;
        let (group_version, child_bytes, family) = parse_jt9_group_data(family)?;
        let child_object_ids =
            match read_jt_object_ids(ctx, child_bytes, "decode DisplayJT group children") {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            };
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
        if file_name.is_empty() {
            return None;
        }
        let mut characters = file_name.chars();
        while !characters.as_str().is_empty() {
            let character = match ctx.next_charged(&mut characters, "validate DisplayJT partition name") {
                Ok(character) => character?,
                Err(error) => return Some(Err(error)),
            };
            if character.is_control() {
                return None;
            }
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
    })?;
    if parsed.is_some() {
        storage.commit()?;
    }
    Ok(parsed)
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

fn parse_jt_f32_vector(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
) -> Result<Option<Vec<FiniteBinary32>>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "DisplayJT range values candidate")?;
    let parsed = storage.with_storage(|| {
    (|| {
        let count = view.u32_le()?;
        let count = view.counted(u64::from(count), 4)?.get();
        let operation = "decode DisplayJT range values";
        let mut ordinals = 0..count;
        let mut values = match ctx.vector_storage(count, operation) {
            Ok(values) => values,
            Err(error) => return Some(Err(error)),
        };
        while ordinals.len() != 0 {
            if let Err(error) = ctx.next_charged(&mut ordinals, operation) {
                return Some(Err(error));
            }
            let value = FiniteBinary32::new(view.f32_le()?)?;
            if let Err(error) = ctx.push_vec(&mut values, value, operation) {
                return Some(Err(error));
            }
        }
        Some(Ok(values))
    })()
    .transpose()
    })?;
    if parsed.is_some() {
        storage.commit()?;
    }
    Ok(parsed)
}

fn parse_jt9_range_lod_node_body(
    ctx: &DecodeContext<'_>,
    body: &[u8],
) -> Result<Option<ParsedJtRangeLodNode>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "DisplayJT range LOD candidate")?;
    let parsed = storage.with_storage(|| {
    (|| {
        let (_, _, _, family) = parse_jt_base_node_body(body, 9)?;
        let (group_version, child_bytes, family) = parse_jt9_group_data(family)?;
        let child_object_ids =
            match read_jt_object_ids(ctx, child_bytes, "decode DisplayJT group children") {
                Ok(value) => value,
                Err(error) => return Some(Err(error)),
            };
        let mut view = View::over_retained(family);
        let lod_version = view.u16_le()?;
        let reserved_values = match parse_jt_f32_vector(ctx, &mut view) {
            Ok(value) => value?,
            Err(error) => return Some(Err(error)),
        };
        let reserved_value = view.i32_le()?;
        let range_version = view.u16_le()?;
        let range_limits = match parse_jt_f32_vector(ctx, &mut view) {
            Ok(value) => value?,
            Err(error) => return Some(Err(error)),
        };
        let range_limits =
            match JtRangeLimits::from_finite(range_limits, &DecodeJtRangeLimitAdmission { ctx }) {
                Ok(limits) => limits.ok()?,
                Err(error) => return Some(Err(error)),
            };
        let center = [
            FiniteBinary32::new(view.f32_le()?)?,
            FiniteBinary32::new(view.f32_le()?)?,
            FiniteBinary32::new(view.f32_le()?)?,
        ];
        if !view.is_empty() {
            return None;
        }
        Some(Ok(ParsedJtRangeLodNode {
            group_version,
            child_object_ids,
            lod_version,
            reserved_values,
            reserved_value,
            range_version,
            range_limits,
            center,
        }))
    })()
    .transpose()
    })?;
    if parsed.is_some() {
        storage.commit()?;
    }
    Ok(parsed)
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
    for (index_ordinal, entry) in ctx
        .admit_iter(&container.entries, "scan DisplayJT index entries")?
        .filter(|entry| entry.name == "/Root/UG_PART/DisplayJT")
        .enumerate()
    {
        let mut storage = ctx.reserve_scoped(0, "DisplayJT index candidate")?;
        let parsed = storage.with_storage(|| -> Result<Option<DisplayJtIndex>, CodecError> {
            let Some((source_offset, byte_len)) = entry.file_span() else {
                return Ok(None);
            };
            let Some(payload) = container.bounded_entry_bytes(ctx, source_offset, byte_len)? else {
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
            let mut rows = Vec::new();
            ctx.reserve_capacity(&mut rows, row_count, "retain DisplayJT index rows")?;
            let mut previous_header_offset = None;
            let mut ordinals = 0..row_count;
            while ordinals.len() != 0 {
                let Some(ordinal) = ctx.next_charged(&mut ordinals, "scan DisplayJT index rows")? else {
                    break;
                };
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
                let id = ctx.format_retained(
                    format_args!("nx:display-jt:index#{index_ordinal}-row-{ordinal}"),
                    "retain DisplayJT index row identity",
                )?;
                ctx.reserve_vec(&mut rows, 1, "admit DisplayJT index rows")?;
                rows.push(DisplayJtIndexRow {
                    id,
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
            let id = ctx.format_retained(
                format_args!("nx:display-jt:index#{index_ordinal}"),
                "retain DisplayJT index identity",
            )?;
            Ok(DisplayJtIndex::new(id, version, rows, source_offset).ok())
        })?;
        if let Some(index) = parsed {
            ctx.reserve_vec(&mut indices, 1, "admit DisplayJT index")?;
            indices.push(index);
            storage.commit()?;
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
    let Some(entry) = crate::native::unique_by(
        ctx,
        &container.entries,
        |entry| Ok(entry.name == "/Root/UG_PART/DisplayJT"),
        "find DisplayJT stream entry",
    )?
    else {
        return Ok(Vec::new());
    };
    let Some((stream_source_offset, stream_byte_len)) = entry.file_span() else {
        return Ok(Vec::new());
    };
    let Some(stream) = container.bounded_entry_bytes(ctx, stream_source_offset, stream_byte_len)?
    else {
        return Ok(Vec::new());
    };
    let [index] = indices else {
        return Ok(Vec::new());
    };
    let mut storage = ctx.reserve_scoped(0, "DisplayJT document candidates")?;
    let documents = storage.with_storage(|| {
    let mut documents = Vec::new();
    let mut row_ordinals = 0..index.rows.len();
    while row_ordinals.len() != 0 {
        let Some(ordinal) = ctx.next_charged(&mut row_ordinals, "scan DisplayJT document")? else {
            break;
        };
        let Some(row) = index.rows.get(ordinal) else {
            break;
        };
        let Ok(document_start) = usize::try_from(row.header_offset) else {
            return Ok(Vec::new());
        };
        let document_end = index.rows.get(ordinal + 1).map_or(stream.len(), |next| {
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
        let Ok(version) = JtVersionField::new(version_field)
        else {
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
        let document_key = ctx
            .rsplit_once(&row.id, "#", "key DisplayJT document")?
            .map_or(row.id.as_str(), |(_, key)| key);
        let mut toc_ordinals = 0..toc_count_usize;
        let mut toc_entries = ctx.vector_storage(toc_count_usize, "admit DisplayJT toc entries")?;
        while toc_ordinals.len() != 0 {
            let Some(ordinal) = ctx.next_charged(&mut toc_ordinals, "scan DisplayJT table of contents")? else {
                break;
            };
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
            ctx.reserve_vec(&mut toc_entries, 1, "admit DisplayJT toc entries")?;
            ctx.charge_entities(1, "admit DisplayJT toc entry")?;
            let id = ctx.format_retained(
                format_args!("nx:display-jt:toc-entry#{document_key}-{ordinal}"),
                "retain DisplayJT toc identity",
            )?;
            toc_entries.push(DisplayJtTocEntry {
                id,
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
        let id = ctx.format_retained(
            format_args!("nx:display-jt:document#{document_key}"),
            "retain DisplayJT document identity",
        )?;
        let index_row =
            ctx.copy_retained_text(&row.id, "retain DisplayJT document index reference")?;
        ctx.reserve_vec(&mut documents, 1, "admit DisplayJT document")?;
        documents.push(DisplayJtDocument {
            id,
            index_row,
            version,
            toc_offset,
            lsg_segment_id,
            toc_entries,
            physical_byte_len: cadmpeg_core::decode::u64_from_index(document.len()),
            source_offset: stream_source_offset
                + cadmpeg_core::decode::u64_from_index(document_start),
        });
    }
    Ok::<_, CodecError>(documents)
    })?;
    if !documents.is_empty() {
        storage.commit()?;
    }
    Ok(documents)
}

/// Decode every segment declared by complete embedded JT documents.
pub(super) fn display_jt_segments(
    ctx: &DecodeContext<'_>,
    container: &Container,
    documents: &[DisplayJtDocument],
) -> Result<Vec<DisplayJtSegment>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "DisplayJT segment candidates")?;
    let segments = storage.with_storage(|| {
    let mut segments = Vec::new();
    let mut remaining_documents = documents.iter();
    while !remaining_documents.as_slice().is_empty() {
        let Some(document) = ctx.next_charged(&mut remaining_documents, "scan DisplayJT segment documents")? else {
            break;
        };
        let document_key = ctx
            .split_once(&document.id, "#", "key DisplayJT segment document")?
            .map_or(document.id.as_str(), |(_, key)| key);
        let Some(bytes) = container.bounded_entry_bytes(
            ctx,
            document.source_offset,
            document.physical_byte_len,
        )?
        else {
            return Ok(Vec::new());
        };
        let mut remaining_entries = document.toc_entries.iter();
        while !remaining_entries.as_slice().is_empty() {
            let Some(entry) = ctx.next_charged(&mut remaining_entries, "scan DisplayJT segments")? else {
                break;
            };
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
                            .and_then(|end| container.data.view().child(start, end))
                    })
                else {
                    return Ok(Vec::new());
                };
                let mut inflated_storage = ctx.reserve_scoped(0, "NX JT inflated storage")?;
                let Some(inflated) =
                    inflated_storage.with_storage(|| inflate_display_jt(ctx, member))?
                else {
                    return Ok(Vec::new());
                };
                Some(DisplayJtCompression {
                    envelope,
                    inflated_sha256: Sha256Digest::digest_for_decode(
                        ctx,
                        &inflated,
                        "retain DisplayJT hash",
                    )?,
                })
            } else {
                None
            };
            ctx.charge_entities(1, "store DisplayJT segment")?;
            ctx.push_vec(
                &mut segments,
                DisplayJtSegment {
                    id: ctx.format_retained(
                        format_args!("nx:display-jt:segment#{document_key}-{}", entry.ordinal),
                        "store DisplayJT segment",
                    )?,
                    document: ctx.copy_retained_text(&document.id, "store DisplayJT segment")?,
                    toc_entry: ctx.copy_retained_text(&entry.id, "store DisplayJT segment")?,
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
                },
                "store DisplayJT segment",
            )?;
        }
    }
    Ok::<_, CodecError>(segments)
    })?;
    if !segments.is_empty() {
        storage.commit()?;
    }
    Ok(segments)
}

/// Decode complete object-element sequences from type-7 shape-LOD segments.
pub(super) fn display_jt_shape_lod_elements(
    ctx: &DecodeContext<'_>,
    container: &Container,
    segments: &[DisplayJtSegment],
) -> Result<Vec<DisplayJtShapeLodElement>, CodecError> {
    const SEGMENT_TAIL: [u8; 6] = [1, 0, 0, 0, 0, 0];
    let mut storage = ctx.reserve_scoped(0, "DisplayJT shape element candidates")?;
    let elements = storage.with_storage(|| {
    let mut elements = Vec::new();
    let mut remaining_segments = segments.iter();
    while !remaining_segments.as_slice().is_empty() {
        let Some(segment) = ctx.next_charged(&mut remaining_segments, "scan DisplayJT shape LOD segments")? else {
            break;
        };
        if segment.segment_type != 7 {
            continue;
        }
        let Some(bytes) = container.bounded_entry_bytes(
            ctx,
            segment.source_offset,
            u64::from(segment.segment_byte_len),
        )?
        else {
            return Ok(Vec::new());
        };
        let payload = &bytes[24..];
        let mut framing_storage = ctx.reserve_scoped(0, "NX JT framing storage")?;
        let Some((parsed, framed_end)) =
            framing_storage.with_storage(|| parse_jt_element_sequence(ctx, payload))?
        else {
            return Ok(Vec::new());
        };
        if payload.get(framed_end..) != Some(SEGMENT_TAIL.as_slice()) {
            return Ok(Vec::new());
        }
        let mut remaining_elements = parsed.iter().enumerate();
        while remaining_elements.len() != 0 {
            let Some((ordinal, element)) = ctx.next_charged(&mut remaining_elements, "store DisplayJT shape element")? else {
                break;
            };
            if element.object_base_type != 4 {
                return Ok(Vec::new());
            }
            ctx.charge_entities(1, "store DisplayJT shape element")?;
            ctx.push_vec(
                &mut elements,
                DisplayJtShapeLodElement {
                    id: ctx.format_retained(
                        format_args!("{}-element-{ordinal}", segment.id),
                        "store DisplayJT shape element",
                    )?,
                    segment: ctx
                        .copy_retained_text(&segment.id, "store DisplayJT shape element")?,
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
                },
                "store DisplayJT shape element",
            )?;
        }
    }
    Ok::<_, CodecError>(elements)
    })?;
    if !elements.is_empty() {
        storage.commit()?;
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
    let mut storage = ctx.reserve_scoped(0, "DisplayJT tri-strip header candidates")?;
    let headers = storage.with_storage(|| {
    let mut headers = Vec::new();
    let mut remaining_elements = elements.iter();
    while !remaining_elements.as_slice().is_empty() {
        let Some(element) = ctx.next_charged(&mut remaining_elements, "scan DisplayJT tri-strip headers")? else { break; };
        if element.object_type_id != TRI_STRIP_LOD_TYPE { continue; }
        let Some(body_start) = element.source_offset.checked_add(25) else {
            return Ok(Vec::new());
        };
        let Some(body) =
            container.bounded_entry_bytes(ctx, body_start, u64::from(element.body_byte_len))?
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
        ctx.charge_entities(1, "nx JT tri-strip headers")?;
        ctx.push_vec(
            &mut headers,
            DisplayJtTriStripLodHeader {
                id: ctx.join_retained(
                    &[element.id.as_str(), "-tri-strip-header"],
                    "",
                    "nx JT tri-strip identity",
                )?,
                element: ctx.join_retained(
                    &[&element.id],
                    "",
                    "nx JT tri-strip element reference",
                )?,
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
            },
            "nx JT tri-strip headers",
        )?;
    }
    Ok::<_, CodecError>(headers)
    })?;
    if !headers.is_empty() { storage.commit()?; }
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
    let mut candidate_storage = ctx.reserve_scoped(0, "nx JT face degree candidates")?;
    let candidate = candidate_storage.with_storage(|| -> Result<
        Option<Vec<DisplayJtInitialFaceDegreeSymbols>>,
        CodecError,
    > {
        let mut vectors = Vec::new();
        let mut remaining_elements = elements.iter();
        while !remaining_elements.as_slice().is_empty() {
            let Some(element) = ctx.next_charged(
                &mut remaining_elements,
                "scan DisplayJT face degree packets",
            )? else {
                break;
            };
            if element.object_type_id != TRI_STRIP_LOD_TYPE {
                continue;
            }
            let Some(body_start) = element.source_offset.checked_add(25) else {
                return Ok(None);
            };
            let Some(body) = container.bounded_entry_bytes(
                ctx,
                body_start,
                u64::from(element.body_byte_len),
            )? else {
                return Ok(None);
            };
            let Some((_, _, _, _, representation)) = parse_jt9_tri_strip_lod_header(body) else {
                return Ok(None);
            };
            let Some((degrees, packet_byte_len)) =
                crate::jt::decode_int32_cdp2(ctx, representation, 0)?
            else {
                return Ok(None);
            };
            let Some(packet) = representation.get(..packet_byte_len) else {
                return Ok(None);
            };
            ctx.charge_entities(1, "nx JT face degree records")?;
            ctx.push_vec(
                &mut vectors,
                DisplayJtInitialFaceDegreeSymbols {
                    id: ctx.join_retained(
                        &[element.id.as_str(), "-initial-face-degrees"],
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
                    packet_sha256: Sha256Digest::digest_for_decode(
                        ctx,
                        packet,
                        "retain DisplayJT hash",
                    )?,
                    source_offset: element.source_offset + 45,
                },
                "nx JT face degree records",
            )?;
        }
        Ok(Some(vectors))
    })?;
    let Some(vectors) = candidate else {
        return Ok(Vec::new());
    };
    if !vectors.is_empty() {
        candidate_storage.commit()?;
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
    let mut candidate_storage = ctx.reserve_scoped(0, "nx JT topology output candidates")?;
    let candidate = candidate_storage.with_storage(|| -> Result<Option<DisplayJtTopologyArrays>, CodecError> {
    let mut sequences = Vec::new();
    let mut headers = Vec::new();
    let mut coordinate_headers = Vec::new();
    let mut remaining_elements = elements.iter();
    while !remaining_elements.as_slice().is_empty() {
        let Some(element) =
            ctx.next_charged(&mut remaining_elements, "scan DisplayJT topology packets")?
        else {
            break;
        };
        if element.object_type_id != TRI_STRIP_LOD_TYPE {
            continue;
        }
        let Some(body_start) = element.source_offset.checked_add(25) else {
            return Ok(None);
        };
        let Some(body) =
            container.bounded_entry_bytes(ctx, body_start, u64::from(element.body_byte_len))?
        else {
            return Ok(None);
        };
        let Some((lod_vertex_bindings, _, _, _, representation)) =
            parse_jt9_tri_strip_lod_header(body)
        else {
            return Ok(None);
        };
        let mut cursor = 0usize;
        let Some(high_degree_lane_count) =
            jt9_topology_high_degree_lane_count(ctx, representation, lod_vertex_bindings)?
        else {
            return Ok(None);
        };
        let Some(role_count) = 23usize.checked_add(high_degree_lane_count) else {
            return Ok(None);
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
        let mut packets = ctx.vector_storage(role_count, "nx JT topology packets")?;
        let mut roles = roles;
        let mut roles_remaining = role_count;
        while roles_remaining != 0 {
            let Some(role) = ctx.next_charged(&mut roles, "nx JT topology packets")? else {
                break;
            };
            roles_remaining -= 1;
            let Some(remaining) = representation.get(cursor..) else {
                return Ok(None);
            };
            let Some((value_count, codec, byte_len)) =
                crate::jt::frame_int32_cdp2(ctx, remaining, 0)?
            else {
                return Ok(None);
            };
            let Some(packet_end) = cursor.checked_add(byte_len) else {
                return Ok(None);
            };
            let Some(packet) = representation.get(cursor..packet_end) else {
                return Ok(None);
            };
            let (Ok(byte_len), Ok(representation_offset)) =
                (u32::try_from(byte_len), u32::try_from(cursor))
            else {
                return Ok(None);
            };
            let mut residual_storage = ctx.reserve_scoped(0, "nx JT topology residuals")?;
            let decoded = residual_storage
                .with_storage(|| crate::jt::decode_int32_cdp2(ctx, packet, 0))?;
            let values = match (decoded, residual_storage) {
                (Some((residuals_candidate, decoded_byte_len)), storage) if decoded_byte_len == packet.len() => {
                    let residuals = residuals_candidate;
                    let predictor = match role {
                        TopologyPacketRole::VertexFlags | TopologyPacketRole::SplitFaceSymbols => {
                            crate::jt::Predictor::Lag1
                        }
                        _ => crate::jt::Predictor::Null,
                    };
                    if predictor == crate::jt::Predictor::Null {
                        storage.commit()?;
                        Some(residuals)
                    } else {
                        let values = crate::jt::unpack_predictor_residuals(
                            ctx, &residuals, predictor,
                        )?;
                        drop(residuals);
                        drop(storage);
                        Some(values)
                    }
                }
                (decoded, storage) => {
                    drop(decoded);
                    drop(storage);
                    None
                }
            };
            ctx.reserve_vec(&mut packets, 1, "nx JT topology packets")?;
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
            return Ok(None);
        };
        cursor += 4;
        let Some(required_header_end) = cursor.checked_add(16) else {
            return Ok(None);
        };
        let Some(required_header) = representation
            .get(cursor..required_header_end)
            .and_then(|bytes| View::over_retained(bytes).array::<16>())
        else {
            return Ok(None);
        };
        let [bindings @ .., q0, q1, q2, q3, n0, n1, n2, n3] = required_header;
        let vertex_bindings = assemble_u64_le(bindings);
        let quantization = [q0, q1, q2, q3];
        if quantization[0] > 24
            || quantization[1] > 13
            || quantization[2] > 24
            || quantization[3] > 24
        {
            return Ok(None);
        }
        if vertex_bindings != lod_vertex_bindings {
            return Ok(None);
        }
        let topological_vertex_count = assemble_u32_le([n0, n1, n2, n3]);
        let (vertex_attribute_count, vertex_header_byte_len) = if topological_vertex_count == 0 {
            (0, 16)
        } else {
            let Some(attribute_end) = cursor.checked_add(20) else {
                return Ok(None);
            };
            let Some(attribute_bytes) = representation
                .get(cursor + 16..attribute_end)
                .and_then(|bytes| View::over_retained(bytes).array::<4>())
            else {
                return Ok(None);
            };
            (assemble_u32_le(attribute_bytes), 20)
        };
        if i32::try_from(topological_vertex_count).is_err()
            || i32::try_from(vertex_attribute_count).is_err()
        {
            return Ok(None);
        }
        let arrays = &representation[cursor + vertex_header_byte_len..];
        let (Ok(topology_byte_len), Ok(compressed_arrays_byte_len)) =
            (u32::try_from(cursor), u32::try_from(arrays.len()))
        else {
            return Ok(None);
        };
        let representation_source_offset = element.source_offset + 45;
        if topological_vertex_count != 0 {
            let Some(coordinate_header) = View::over_retained(arrays).array::<32>() else {
                return Ok(None);
            };
            let [n0, n1, n2, n3, component_count, ranges @ ..] = coordinate_header;
            let unique_vertex_count = assemble_u32_le([n0, n1, n2, n3]);
            if unique_vertex_count != topological_vertex_count || component_count != 3 {
                return Ok(None);
            }
            let mut component_ranges = [QuantizedRange::ZERO; 3];
            let mut component_quantization_bits = [0; 3];
            let Ok(components) = <[[u8; 9]; 3]>::try_from(ranges.as_chunks::<9>().0) else {
                return Ok(None);
            };
            for (component, [m0, m1, m2, m3, x0, x1, x2, x3, bits]) in
                components.into_iter().enumerate()
            {
                let minimum = assemble_f32_le([m0, m1, m2, m3]);
                let maximum = assemble_f32_le([x0, x1, x2, x3]);
                let Some(range) = QuantizedRange::new(minimum, maximum) else {
                    return Ok(None);
                };
                if bits > 32 || bits != quantization[0] {
                    return Ok(None);
                }
                component_ranges[component] = range;
                component_quantization_bits[component] = bits;
            }
            let compressed_components = &arrays[32..];
            let Ok(compressed_components_byte_len) = u32::try_from(compressed_components.len())
            else {
                return Ok(None);
            };
            let Ok(vertex_header_byte_len_u64) = u64::try_from(vertex_header_byte_len) else {
                return Ok(None);
            };
            ctx.charge_entities(1, "nx JT coordinate headers")?;
            ctx.push_vec(
                &mut coordinate_headers,
                DisplayJtVertexCoordinateArrayHeader {
                    id: ctx.join_retained(
                        &[element.id.as_str(), "-coordinate-array-header"],
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
                },
                "nx JT coordinate headers",
            )?;
        }
        ctx.charge_entities(1, "nx JT topology sequences")?;
        ctx.push_vec(
            &mut sequences,
            DisplayJtTopologyPacketSequence {
                id: ctx.join_retained(
                    &[element.id.as_str(), "-topology-packets"],
                    "",
                    "nx JT topology sequence identity",
                )?,
                element: ctx.join_retained(&[&element.id], "", "nx JT element reference")?,
                packets,
                composite_hash,
                topology_byte_len,
                source_offset: representation_source_offset,
            },
            "nx JT topology sequences",
        )?;
        ctx.charge_entities(1, "nx JT vertex headers")?;
        ctx.push_vec(
            &mut headers,
            DisplayJtCompressedVertexRecordsHeader {
                id: ctx.join_retained(
                    &[element.id.as_str(), "-vertex-records-header"],
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
            },
            "nx JT vertex headers",
        )?;
    }
    Ok(Some(DisplayJtTopologyArrays {
        sequences,
        vertex_headers: headers,
        coordinate_headers,
    }))
    })?;
    let Some(arrays) = candidate else {
        return Ok(DisplayJtTopologyArrays::default());
    };
    if !arrays.sequences.is_empty()
        || !arrays.vertex_headers.is_empty()
        || !arrays.coordinate_headers.is_empty()
    {
        candidate_storage.commit()?;
    }
    Ok(arrays)
}

/// Decode every complete JT 9 coordinate array.
pub(super) fn display_jt_vertex_coordinates(
    ctx: &DecodeContext<'_>,
    container: &Container,
    headers: &[DisplayJtVertexCoordinateArrayHeader],
) -> Result<Vec<DisplayJtVertexCoordinates>, CodecError> {
    let mut candidate_storage = ctx.reserve_scoped(0, "nx JT coordinate output candidates")?;
    let candidate = candidate_storage.with_storage(|| -> Result<
        Option<Vec<DisplayJtVertexCoordinates>>,
        CodecError,
    > {
    let mut arrays = Vec::new();
    let mut remaining_headers = headers.iter();
    while !remaining_headers.as_slice().is_empty() {
        let Some(header) =
            ctx.next_charged(&mut remaining_headers, "scan DisplayJT coordinate headers")?
        else {
            break;
        };
        let Some(start) = header.source_offset.checked_add(32) else {
            return Ok(None);
        };
        let Some(bytes) = container.bounded_entry_bytes(
            ctx,
            start,
            u64::from(header.compressed_components_byte_len),
        )?
        else {
            return Ok(None);
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
            return Ok(None);
        };
        let Ok(consumed) = u32::try_from(consumed) else {
            return Ok(None);
        };
        ctx.charge_entities(1, "nx JT vertex coordinates")?;
        ctx.push_vec(
            &mut arrays,
            DisplayJtVertexCoordinates {
                id: replace_jt_text_once(
                    ctx,
                    &header.id,
                    "coordinate-array-header",
                    "vertex-coordinates",
                    "nx JT coordinate identity",
                )?,
                header: ctx.join_retained(
                    &[&header.id],
                    "",
                    "nx JT coordinate header reference",
                )?,
                points_m,
                coordinate_hash,
                byte_len: consumed,
                source_offset: header.source_offset + 32,
            },
            "nx JT vertex coordinates",
        )?;
    }
    Ok(Some(arrays))
    })?;
    let Some(arrays) = candidate else {
        return Ok(Vec::new());
    };
    if !arrays.is_empty() {
        candidate_storage.commit()?;
    }
    Ok(arrays)
}

/// Reconstruct every complete JT 9 polygon mesh from its dual-mesh lanes.
pub(super) fn display_jt_polygon_meshes(
    ctx: &DecodeContext<'_>,
    sequences: &[DisplayJtTopologyPacketSequence],
    coordinate_headers: &[DisplayJtVertexCoordinateArrayHeader],
) -> Result<Vec<DisplayJtPolygonMesh>, CodecError> {
    let (indexed_headers_by_element, headers_storage) = ctx.unique_index(
        coordinate_headers
            .iter()
            .map(|header| (header.element.as_str(), header)),
        "index DisplayJT coordinate headers",
    )?;
    let _headers_storage = headers_storage;
    let headers_by_element = indexed_headers_by_element;
    let mut candidate_storage = ctx.reserve_scoped(0, "nx JT polygon output candidates")?;
    let candidate = candidate_storage.with_storage(|| -> Result<Option<Vec<DisplayJtPolygonMesh>>, CodecError> {
    let mut meshes = Vec::new();
    let mut remaining_sequences = sequences.iter();
    while !remaining_sequences.as_slice().is_empty() {
        let Some(sequence) =
            ctx.next_charged(&mut remaining_sequences, "scan DisplayJT topology sequences")?
        else {
            break;
        };
        let values = |role: TopologyPacketRole| -> Result<Option<&[i32]>, CodecError> {
            Ok(ctx
                .find_by(
                    &sequence.packets,
                    |packet| Ok(packet.role == role),
                    "find DisplayJT topology lane",
                )?
                .and_then(|packet| packet.values.as_deref()))
        };
        let Some(valences) = values(TopologyPacketRole::VertexValences)? else {
            return Ok(None);
        };
        if valences.is_empty() {
            continue;
        }
        let Some(&Some(coordinate_header)) = ctx.get_hash_map(
            &headers_by_element,
            sequence.element.as_str(),
            "match DisplayJT coordinate headers",
        )?
        else {
            return Ok(None);
        };
        let mut degrees = [&[][..]; 8];
        for (lane_index, context) in TopologyContext::ALL.into_iter().enumerate() {
            let Some(lane) = values(TopologyPacketRole::FaceDegrees(context))? else {
                return Ok(None);
            };
            degrees[lane_index] = lane;
        }
        let mut attribute_masks = [&[][..]; 8];
        for (lane_index, context) in TopologyContext::ALL.into_iter().enumerate() {
            let Some(lane) = values(TopologyPacketRole::FaceAttributeMasks(context))? else {
                return Ok(None);
            };
            attribute_masks[lane_index] = lane;
        }
        let Some(context_7_next_30) = values(TopologyPacketRole::FaceAttributeMasks7Next30)? else {
            return Ok(None);
        };
        let Some(context_7_upper_4) = values(TopologyPacketRole::FaceAttributeMasks7Upper4)? else {
            return Ok(None);
        };
        let mut large_lane_storage = ctx.reserve_scoped(0, "nx JT large mask lanes")?;
        let mut large_lanes = Vec::new();
        let mut large_word_count = 0usize;
        let mut remaining_packets = sequence.packets.iter();
        while !remaining_packets.as_slice().is_empty() {
            let Some(packet) =
                ctx.next_charged(&mut remaining_packets, "nx JT large mask lanes")?
            else {
                break;
            };
            if !matches!(
                packet.role,
                TopologyPacketRole::HighDegreeFaceAttributeMasks(_)
            ) {
                continue;
            }
            let Some(lane) = packet.values.as_deref() else {
                return Ok(None);
            };
            let Some(count) = large_word_count.checked_add(lane.len()) else {
                return Ok(None);
            };
            large_word_count = count;
            ctx.push_scoped_vec(
                &mut large_lane_storage,
                &mut large_lanes,
                lane,
                "nx JT large mask lanes",
            )?;
        }
        let (large_words, large_word_storage) =
            ctx.scoped_vector_storage(large_word_count, "nx JT large mask words")?;
        let mut large_word_storage = large_word_storage;
        let mut large_words = large_words;
        let mut remaining_lanes = large_lanes.iter();
        while !remaining_lanes.as_slice().is_empty() {
            let Some(lane) = ctx.next_charged(&mut remaining_lanes, "nx JT large mask words")? else {
                break;
            };
            large_word_storage.with_storage(|| {
                ctx.extend_from_slice(&mut large_words, lane, "nx JT large mask words")
            })?;
        }
        drop(large_lanes);
        drop(large_lane_storage);
        let decoded_polygons = crate::jt_topology::decode(
            ctx,
            degrees,
            valences,
            values(TopologyPacketRole::VertexGroups)?.unwrap_or_default(),
            values(TopologyPacketRole::VertexFlags)?.unwrap_or_default(),
            crate::jt_topology::SplitLanes {
                faces: values(TopologyPacketRole::SplitFaceSymbols)?.unwrap_or_default(),
                positions: values(TopologyPacketRole::SplitFacePositions)?.unwrap_or_default(),
            },
            crate::jt_topology::AttributeMaskLanes {
                small: attribute_masks,
                context_7_next_30,
                context_7_upper_4,
                large_words: &large_words,
            },
        )?;
        drop(large_words);
        drop(large_word_storage);
        let Some(polygons) = decoded_polygons else {
            return Ok(None);
        };
        if ctx.any_by(
            &polygons,
            |polygon| {
                ctx.any_by(
                    &polygon.corners,
                    |&(index, _)| Ok(index >= coordinate_header.unique_vertex_count),
                    "validate DisplayJT polygon corners",
                )
            },
            "validate DisplayJT polygons",
        )? {
            return Ok(None);
        }
        ctx.charge_entities(1, "nx JT polygon meshes")?;
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
        ctx.push_vec(&mut meshes, mesh, "nx JT polygon meshes")?;
    }
    Ok(Some(meshes))
    })?;
    let Some(meshes) = candidate else {
        return Ok(Vec::new());
    };
    if !meshes.is_empty() {
        candidate_storage.commit()?;
    }
    Ok(meshes)
}

/// Lookup tables over the decoded vertex arrays that precede one vertex array.
///
/// Each table keys one owner reference. Identities are unique in decoded
/// arrays, so a repeated key matches no record.
struct JtVertexArrayIndex<'a, 'ctx> {
    coordinate_headers: HashMap<&'a str, Option<&'a DisplayJtVertexCoordinateArrayHeader>>,
    coordinates: HashMap<&'a str, Option<&'a DisplayJtVertexCoordinates>>,
    normals: HashMap<&'a str, Option<&'a DisplayJtVertexNormals>>,
    colors: HashMap<&'a str, Option<&'a DisplayJtVertexColors>>,
    texture_coordinates: HashMap<(&'a str, u8), Option<&'a DisplayJtVertexTextureCoordinates>>,
    _storage: [ScopedReservation<'ctx>; 5],
}

impl<'a, 'ctx> JtVertexArrayIndex<'a, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        coordinate_headers: &'a [DisplayJtVertexCoordinateArrayHeader],
        coordinates: &'a [DisplayJtVertexCoordinates],
        normals: &'a [DisplayJtVertexNormals],
        colors: &'a [DisplayJtVertexColors],
        texture_coordinates: &'a [DisplayJtVertexTextureCoordinates],
    ) -> Result<Self, CodecError> {
        let (coordinate_headers_candidate, coordinate_header_storage_candidate) = ctx.unique_index(
            coordinate_headers
                .iter()
                .map(|header| (header.element.as_str(), header)),
            "index DisplayJT coordinate headers",
        )?;
        let coordinate_header_storage = coordinate_header_storage_candidate;
        let coordinate_headers = coordinate_headers_candidate;
        let (coordinates_candidate, coordinate_storage_candidate) = ctx.unique_index(
            coordinates
                .iter()
                .map(|array| (array.header.as_str(), array)),
            "index DisplayJT vertex coordinates",
        )?;
        let coordinate_storage = coordinate_storage_candidate;
        let coordinates = coordinates_candidate;
        let (normals_candidate, normal_storage_candidate) = ctx.unique_index(
            normals
                .iter()
                .map(|array| (array.vertex_records_header.as_str(), array)),
            "index DisplayJT vertex normals",
        )?;
        let normal_storage = normal_storage_candidate;
        let normals = normals_candidate;
        let (colors_candidate, color_storage_candidate) = ctx.unique_index(
            colors
                .iter()
                .map(|array| (array.vertex_records_header.as_str(), array)),
            "index DisplayJT vertex colors",
        )?;
        let color_storage = color_storage_candidate;
        let colors = colors_candidate;
        let (texture_coordinates_candidate, texture_storage_candidate) = ctx.unique_index(
            texture_coordinates
                .iter()
                .map(|array| ((array.vertex_records_header.as_str(), array.channel), array)),
            "index DisplayJT texture coordinates",
        )?;
        let texture_storage = texture_storage_candidate;
        let texture_coordinates = texture_coordinates_candidate;
        Ok(Self {
            coordinate_headers,
            coordinates,
            normals,
            colors,
            texture_coordinates,
            _storage: [
                coordinate_header_storage,
                coordinate_storage,
                normal_storage,
                color_storage,
                texture_storage,
            ],
        })
    }

    /// Source offset of the first byte after the coordinate array of a vertex header.
    fn coordinates_end(
        &self,
        ctx: &DecodeContext<'_>,
        vertex_header: &DisplayJtCompressedVertexRecordsHeader,
    ) -> Result<Option<u64>, CodecError> {
        let Some(&Some(coordinate_header)) = ctx.get_hash_map(
            &self.coordinate_headers,
            vertex_header.element.as_str(),
            "match DisplayJT coordinate headers",
        )?
        else {
            return Ok(None);
        };
        let Some(&Some(coordinates)) = ctx.get_hash_map(
            &self.coordinates,
            coordinate_header.id.as_str(),
            "match DisplayJT vertex coordinates",
        )?
        else {
            return Ok(None);
        };
        Ok(coordinates
            .source_offset
            .checked_add(u64::from(coordinates.byte_len)))
    }

    fn normals(
        &self,
        ctx: &DecodeContext<'_>,
        vertex_header: &DisplayJtCompressedVertexRecordsHeader,
    ) -> Result<Option<&'a DisplayJtVertexNormals>, CodecError> {
        Ok(ctx
            .get_hash_map(
                &self.normals,
                vertex_header.id.as_str(),
                "match DisplayJT vertex normals",
            )?
            .copied()
            .flatten())
    }

    fn colors(
        &self,
        ctx: &DecodeContext<'_>,
        vertex_header: &DisplayJtCompressedVertexRecordsHeader,
    ) -> Result<Option<&'a DisplayJtVertexColors>, CodecError> {
        Ok(ctx
            .get_hash_map(
                &self.colors,
                vertex_header.id.as_str(),
                "match DisplayJT vertex colors",
            )?
            .copied()
            .flatten())
    }

    fn texture_coordinates(
        &self,
        ctx: &DecodeContext<'_>,
        vertex_header: &DisplayJtCompressedVertexRecordsHeader,
        channel: u8,
    ) -> Result<Option<&'a DisplayJtVertexTextureCoordinates>, CodecError> {
        Ok(ctx
            .get_hash_map(
                &self.texture_coordinates,
                &(vertex_header.id.as_str(), channel),
                "match DisplayJT texture coordinates",
            )?
            .copied()
            .flatten())
    }

    /// Source offset after the coordinate array and the bound normal and color
    /// arrays that precede the attribute arrays of a vertex header.
    fn colors_end(
        &self,
        ctx: &DecodeContext<'_>,
        vertex_header: &DisplayJtCompressedVertexRecordsHeader,
    ) -> Result<Option<u64>, CodecError> {
        let Some(mut source_offset) = self.coordinates_end(ctx, vertex_header)? else {
            return Ok(None);
        };
        if vertex_header.vertex_bindings & 0x8 != 0 {
            let Some(normal_array) = self.normals(ctx, vertex_header)? else {
                return Ok(None);
            };
            let Some(next) = source_offset.checked_add(u64::from(normal_array.byte_len)) else {
                return Ok(None);
            };
            source_offset = next;
        }
        if vertex_header.vertex_bindings & 0x30 != 0 {
            let Some(color_array) = self.colors(ctx, vertex_header)? else {
                return Ok(None);
            };
            let Some(next) = source_offset.checked_add(u64::from(color_array.byte_len)) else {
                return Ok(None);
            };
            source_offset = next;
        }
        Ok(Some(source_offset))
    }
}

/// Decode every complete JT 9 normal array following a coordinate array.
pub(super) fn display_jt_vertex_normals(
    ctx: &DecodeContext<'_>,
    container: &Container,
    vertex_headers: &[DisplayJtCompressedVertexRecordsHeader],
    coordinate_headers: &[DisplayJtVertexCoordinateArrayHeader],
    coordinates: &[DisplayJtVertexCoordinates],
) -> Result<Vec<DisplayJtVertexNormals>, CodecError> {
    let index = JtVertexArrayIndex::new(ctx, coordinate_headers, coordinates, &[], &[], &[])?;
    let mut candidate_storage = ctx.reserve_scoped(0, "nx JT normal output candidates")?;
    let candidate = candidate_storage.with_storage(|| -> Result<Option<Vec<DisplayJtVertexNormals>>, CodecError> {
    let mut arrays = Vec::new();
    let mut remaining_headers = vertex_headers.iter();
    while !remaining_headers.as_slice().is_empty() {
        let Some(vertex_header) =
            ctx.next_charged(&mut remaining_headers, "scan DisplayJT normal headers")?
        else {
            break;
        };
        if vertex_header.vertex_attribute_count == 0 || vertex_header.vertex_bindings & 0x8 == 0 {
            continue;
        }
        let Some(source_offset) = index.coordinates_end(ctx, vertex_header)? else {
            return Ok(None);
        };
        let Some(bytes) = container.bounded_entry_tail(ctx, source_offset)? else {
            return Ok(None);
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
            return Ok(None);
        };
        let Ok(byte_len) = u32::try_from(byte_len) else {
            return Ok(None);
        };
        ctx.charge_entities(1, "nx JT vertex normals")?;
        ctx.push_vec(
            &mut arrays,
            DisplayJtVertexNormals {
                id: ctx.join_retained(
                    &[vertex_header.element.as_str(), "-vertex-normals"],
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
            },
            "nx JT vertex normals",
        )?;
    }
    Ok(Some(arrays))
    })?;
    let Some(arrays) = candidate else {
        return Ok(Vec::new());
    };
    if !arrays.is_empty() {
        candidate_storage.commit()?;
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
    let index = JtVertexArrayIndex::new(ctx, coordinate_headers, coordinates, normals, &[], &[])?;
    let mut candidate_storage = ctx.reserve_scoped(0, "nx JT color output candidates")?;
    let candidate = candidate_storage.with_storage(|| -> Result<Option<Vec<DisplayJtVertexColors>>, CodecError> {
    let mut arrays = Vec::new();
    let mut remaining_headers = vertex_headers.iter();
    while !remaining_headers.as_slice().is_empty() {
        let Some(vertex_header) =
            ctx.next_charged(&mut remaining_headers, "scan DisplayJT color headers")?
        else {
            break;
        };
        if vertex_header.vertex_attribute_count == 0 || vertex_header.vertex_bindings & 0x30 == 0 {
            continue;
        }
        let Some(mut source_offset) = index.coordinates_end(ctx, vertex_header)? else {
            return Ok(None);
        };
        if vertex_header.vertex_bindings & 0x8 != 0 {
            let Some(normal_array) = index.normals(ctx, vertex_header)? else {
                return Ok(None);
            };
            let Some(next) = source_offset.checked_add(u64::from(normal_array.byte_len)) else {
                return Ok(None);
            };
            source_offset = next;
        }
        let Some(bytes) = container.bounded_entry_tail(ctx, source_offset)? else {
            return Ok(None);
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
            return Ok(None);
        };
        let Ok(byte_len) = u32::try_from(byte_len) else {
            return Ok(None);
        };
        ctx.charge_entities(1, "nx JT vertex colors")?;
        ctx.push_vec(
            &mut arrays,
            DisplayJtVertexColors {
                id: ctx.join_retained(
                    &[vertex_header.element.as_str(), "-vertex-colors"],
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
            },
            "nx JT vertex colors",
        )?;
    }
    Ok(Some(arrays))
    })?;
    let Some(arrays) = candidate else {
        return Ok(Vec::new());
    };
    if !arrays.is_empty() {
        candidate_storage.commit()?;
    }
    Ok(arrays)
}

/// Whether a vertex-binding mask binds one of the eight texture-coordinate channels.
fn jt_texture_channel_bound(vertex_bindings: u64, channel: u8) -> bool {
    vertex_bindings & (0xf_u64 << (8 + 4 * channel)) != 0
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
    let index =
        JtVertexArrayIndex::new(ctx, coordinate_headers, coordinates, normals, colors, &[])?;
    let mut candidate_storage = ctx.reserve_scoped(0, "nx JT texture output candidates")?;
    let candidate = candidate_storage.with_storage(|| -> Result<
        Option<Vec<DisplayJtVertexTextureCoordinates>>,
        CodecError,
    > {
    let mut arrays = Vec::new();
    let mut remaining_headers = vertex_headers.iter();
    while !remaining_headers.as_slice().is_empty() {
        let Some(vertex_header) =
            ctx.next_charged(&mut remaining_headers, "scan DisplayJT texture headers")?
        else {
            break;
        };
        if !(0..8_u8)
            .any(|channel| jt_texture_channel_bound(vertex_header.vertex_bindings, channel))
        {
            continue;
        }
        if vertex_header.vertex_attribute_count == 0 {
            return Ok(None);
        }
        let Some(mut source_offset) = index.colors_end(ctx, vertex_header)? else {
            return Ok(None);
        };
        for channel in 0..8_u8 {
            if !jt_texture_channel_bound(vertex_header.vertex_bindings, channel) {
                continue;
            }
            let Some(bytes) = container.bounded_entry_tail(ctx, source_offset)? else {
                return Ok(None);
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
                return Ok(None);
            };
            let Ok(byte_len) = u32::try_from(byte_len) else {
                return Ok(None);
            };
            ctx.charge_entities(1, "nx JT texture coordinates")?;
            ctx.push_vec(
                &mut arrays,
                DisplayJtVertexTextureCoordinates {
                    id: ctx.join_retained(
                        &[
                            vertex_header.element.as_str(),
                            "-texture-coordinates-",
                            ["0", "1", "2", "3", "4", "5", "6", "7"][usize::from(channel)],
                        ],
                        "",
                        "nx JT texture identity",
                    )?,
                    vertex_records_header: ctx.join_retained(
                        &[&vertex_header.id],
                        "",
                        "nx JT texture header reference",
                    )?,
                    channel,
                    values,
                    texture_coordinate_hash,
                    byte_len,
                    source_offset,
                },
                "nx JT texture coordinates",
            )?;
            let Some(next) = source_offset.checked_add(u64::from(byte_len)) else {
                return Ok(None);
            };
            source_offset = next;
        }
    }
    Ok(Some(arrays))
    })?;
    let Some(arrays) = candidate else {
        return Ok(Vec::new());
    };
    if !arrays.is_empty() {
        candidate_storage.commit()?;
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
    let index = JtVertexArrayIndex::new(
        ctx,
        coordinate_headers,
        coordinates,
        normals,
        colors,
        texture_coordinates,
    )?;
    let mut candidate_storage = ctx.reserve_scoped(0, "nx JT flag output candidates")?;
    let candidate = candidate_storage.with_storage(|| -> Result<Option<Vec<DisplayJtVertexFlags>>, CodecError> {
    let mut arrays = Vec::new();
    let mut remaining_headers = vertex_headers.iter();
    while !remaining_headers.as_slice().is_empty() {
        let Some(vertex_header) =
            ctx.next_charged(&mut remaining_headers, "scan DisplayJT flag headers")?
        else {
            break;
        };
        if vertex_header.vertex_attribute_count == 0 || vertex_header.vertex_bindings & 0x40 == 0 {
            continue;
        }
        let Some(mut source_offset) = index.colors_end(ctx, vertex_header)? else {
            return Ok(None);
        };
        for channel in 0..8_u8 {
            if !jt_texture_channel_bound(vertex_header.vertex_bindings, channel) {
                continue;
            }
            let Some(array) = index.texture_coordinates(ctx, vertex_header, channel)? else {
                return Ok(None);
            };
            let Some(next) = source_offset.checked_add(u64::from(array.byte_len)) else {
                return Ok(None);
            };
            source_offset = next;
        }
        let Some(bytes) = container.bounded_entry_tail(ctx, source_offset)? else {
            return Ok(None);
        };
        let Some((values, byte_len)) = crate::jt::decode_vertex_flags(
            ctx,
            bytes,
            cadmpeg_core::decode::index_from_u32(vertex_header.vertex_attribute_count),
        )?
        else {
            return Ok(None);
        };
        let Ok(byte_len) = u32::try_from(byte_len) else {
            return Ok(None);
        };
        ctx.charge_entities(1, "nx JT vertex flags")?;
        ctx.push_vec(
            &mut arrays,
            DisplayJtVertexFlags {
                id: ctx.join_retained(
                    &[vertex_header.element.as_str(), "-vertex-flags"],
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
            },
            "nx JT vertex flags",
        )?;
    }
    Ok(Some(arrays))
    })?;
    let Some(arrays) = candidate else {
        return Ok(Vec::new());
    };
    if !arrays.is_empty() {
        candidate_storage.commit()?;
    }
    Ok(arrays)
}

/// Inflates the compressed payload of one segment under a scoped reservation.
fn inflate_display_jt_segment<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    container: &Container,
    segment: &DisplayJtSegment,
) -> Result<Option<(Vec<u8>, ScopedReservation<'ctx>)>, CodecError> {
    let Some(bytes) = container.bounded_entry_bytes(
        ctx,
        segment.source_offset,
        u64::from(segment.segment_byte_len),
    )?
    else {
        return Ok(None);
    };
    let Some(compressed) = bytes.get(33..) else {
        return Ok(None);
    };
    let Some(member) = segment
        .source_offset
        .checked_add(33)
        .and_then(cadmpeg_core::decode::index_from_u64)
        .and_then(|start| {
            start
                .checked_add(compressed.len())
                .and_then(|end| container.data.view().child(start, end))
        })
    else {
        return Ok(None);
    };
    let mut inflated_storage = ctx.reserve_scoped(0, "NX JT inflated storage")?;
    let Some(inflated) = inflated_storage.with_storage(|| inflate_display_jt(ctx, member))? else {
        return Ok(None);
    };
    Ok(Some((inflated, inflated_storage)))
}

/// Frames the object elements of a payload under a scoped reservation.
type FramedJtElements<'a, 'ctx> = (Vec<ParsedJtElement<'a>>, usize, ScopedReservation<'ctx>);

enum DisplayJtCandidateFailure {
    Codec(CodecError),
    Framing(&'static str),
}

impl From<CodecError> for DisplayJtCandidateFailure {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

fn frame_display_jt_elements<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    payload: &'a [u8],
    operation: &'static str,
) -> Result<Option<FramedJtElements<'a, 'ctx>>, CodecError> {
    let mut framing_storage = ctx.reserve_scoped(0, operation)?;
    let Some((elements, framed_end)) =
        framing_storage.with_storage(|| parse_jt_element_sequence(ctx, payload))?
    else {
        return Ok(None);
    };
    Ok(Some((elements, framed_end, framing_storage)))
}

/// Decode element framing and exact post-marker tails from compressed segments.
pub(super) fn display_jt_compressed_element_sequences(
    ctx: &DecodeContext<'_>,
    container: &Container,
    segments: &[DisplayJtSegment],
) -> Result<
    (
        Vec<DisplayJtCompressedElement>,
        Vec<DisplayJtCompressedElementSequence>,
    ),
    cadmpeg_core::CodecError,
> {
    let mut candidate_storage = ctx.reserve_scoped(0, "nx JT compressed sequence candidates")?;
    let candidate = candidate_storage.with_storage(|| -> Result<
        Option<(
            Vec<DisplayJtCompressedElement>,
            Vec<DisplayJtCompressedElementSequence>,
        )>,
        DisplayJtCandidateFailure,
    > {
    let mut elements = Vec::new();
    let mut sequences = Vec::new();
    let mut remaining_segments = segments.iter();
    while !remaining_segments.as_slice().is_empty() {
        let Some(segment) =
            ctx.next_charged(&mut remaining_segments, "scan DisplayJT compressed segments")?
        else {
            break;
        };
        if segment.compression.is_none() {
            continue;
        }
        let Some((inflated_candidate, inflated_storage)) =
            inflate_display_jt_segment(ctx, container, segment)?
        else {
            return Ok(None);
        };
        let _inflated_storage = inflated_storage;
        let inflated = inflated_candidate;
        let Some((parsed_candidate, framed_end, framing_storage)) =
            frame_display_jt_elements(ctx, &inflated, "NX JT framing storage")?
        else {
            return Ok(None);
        };
        let _framing_storage = framing_storage;
        let parsed = parsed_candidate;
        let mut element_ids = Vec::new();
        ctx.reserve_capacity(
            &mut element_ids,
            parsed.len(),
            "retain DisplayJT element ids",
        )?;
        ctx.reserve_capacity(
            &mut elements,
            parsed.len(),
            "retain DisplayJT compressed elements",
        )?;
        let mut remaining_elements = parsed.iter().enumerate();
        while remaining_elements.len() != 0 {
            let Some((ordinal, element)) =
                ctx.next_charged(&mut remaining_elements, "store DisplayJT compressed elements")?
            else {
                break;
            };
            ctx.reserve_vec(&mut element_ids, 1, "store DisplayJT element ids")?;
            ctx.reserve_vec(&mut elements, 1, "store DisplayJT compressed elements")?;
            let fields = "retain DisplayJT compressed element fields";
            let id = ctx.format_retained(
                format_args!("{}-inflated-element-{ordinal}", segment.id),
                fields,
            )?;
            element_ids.push(ctx.copy_retained_text(&id, fields)?);
            elements.push(
                DisplayJtCompressedElement::try_from(DisplayJtCompressedElementWire {
                    id,
                    segment: ctx.copy_retained_text(&segment.id, fields)?,
                    segment_type: segment.segment_type,
                    ordinal: u32::try_from(ordinal)
                        .map_err(|_| DisplayJtCandidateFailure::Framing("ordinal exceeds u32"))?,
                    object_type_id: element.object_type_id,
                    object_id: element.object_id,
                    object_base_type: element.object_base_type,
                    body_byte_len: u32::try_from(element.body.len())
                        .map_err(|_| {
                            DisplayJtCandidateFailure::Framing("body_byte_len exceeds u32")
                        })?,
                    body_sha256: Sha256Digest::digest_for_decode(
                        ctx,
                        element.body,
                        "hash DisplayJT compressed element body",
                    )?,
                    inflated_offset: u32::try_from(element.offset).map_err(|_| {
                        DisplayJtCandidateFailure::Framing("inflated_offset exceeds u32")
                    })?,
                    source_offset: segment.source_offset + 24,
                })
                .map_err(DisplayJtCandidateFailure::Framing)?,
            );
        }
        let tail = &inflated[framed_end..];
        ctx.charge_collection_items(1, "store DisplayJT compressed sequence")?;
        let fields = "retain DisplayJT compressed sequence fields";
        let id = ctx.format_retained(format_args!("{}-inflated-sequence", segment.id), fields)?;
        let owner = ctx.copy_retained_text(&segment.id, fields)?;
        ctx.reserve_capacity(&mut sequences, 1, "retain DisplayJT compressed sequence")?;
        let retained_tail = ctx.copy_retained(tail, "retain DisplayJT compressed sequence tail")?;
        sequences.push(
            DisplayJtCompressedElementSequence::with_tail_digest(
                DisplayJtCompressedElementSequenceWire {
                    id,
                    segment: owner,
                    segment_type: segment.segment_type,
                    elements: element_ids,
                    framed_byte_len: u32::try_from(framed_end).map_err(|_| {
                        DisplayJtCandidateFailure::Framing("framed_byte_len exceeds u32")
                    })?,
                    tail: retained_tail,
                    tail_sha256: Sha256Digest::digest_for_decode(
                        ctx,
                        tail,
                        "hash DisplayJT compressed sequence tail",
                    )?,
                    source_offset: segment.source_offset + 24,
                },
            )
            .map_err(DisplayJtCandidateFailure::Framing)?,
        );
    }
    Ok(Some((elements, sequences)))
    });
    match candidate {
        Ok(Some(output)) => {
            if !output.0.is_empty() || !output.1.is_empty() {
                candidate_storage.commit()?;
            }
            Ok(output)
        }
        Ok(None) => Ok((Vec::new(), Vec::new())),
        Err(DisplayJtCandidateFailure::Codec(error)) => Err(error),
        Err(DisplayJtCandidateFailure::Framing(message)) => {
            drop(candidate_storage);
            Err(display_jt_framing_error(ctx, message))
        }
    }
}

fn display_jt_framing_error(ctx: &DecodeContext<'_>, message: &str) -> CodecError {
    match ctx.format_retained(
        format_args!(
            "{}: {message}",
            crate::loss::NxLossCode::DisplayJtGraphRejected.code()
        ),
        "retain DisplayJT framing rejection",
    ) {
        Ok(message) => CodecError::Malformed(message),
        Err(error) => error,
    }
}

/// Decode all string property atoms from complete type-31 segment sequences.
pub(super) fn display_jt_string_property_atoms(
    ctx: &DecodeContext<'_>,
    container: &Container,
    segments: &[DisplayJtSegment],
) -> Result<Vec<DisplayJtStringPropertyAtom>, CodecError> {
    const STRING_PROPERTY_ATOM_TYPE: [u8; 16] = [
        0x6e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let mut candidate_storage = ctx.reserve_scoped(0, "nx JT property atom candidates")?;
    let candidate = candidate_storage.with_storage(|| -> Result<
        Option<Vec<DisplayJtStringPropertyAtom>>,
        CodecError,
    > {
    let mut atoms = Vec::new();
    let mut remaining_segments = segments.iter();
    while !remaining_segments.as_slice().is_empty() {
        let Some(segment) =
            ctx.next_charged(&mut remaining_segments, "scan DisplayJT property segments")?
        else {
            break;
        };
        if segment.segment_type != 31 {
            continue;
        }
        if segment.compression.is_none() {
            return Ok(None);
        }
        let Some((inflated_candidate, inflated_storage)) =
            inflate_display_jt_segment(ctx, container, segment)?
        else {
            return Ok(None);
        };
        let _inflated_storage = inflated_storage;
        let inflated = inflated_candidate;
        let Some((elements_candidate, _, framing_storage)) =
            frame_display_jt_elements(ctx, &inflated, "NX JT framing storage")?
        else {
            return Ok(None);
        };
        let _framing_storage = framing_storage;
        let elements = elements_candidate;
        let mut remaining_elements = elements.iter().enumerate();
        while remaining_elements.len() != 0 {
            let Some((ordinal, element)) = ctx.next_charged(
                &mut remaining_elements,
                "store DisplayJT string property atom",
            )? else {
                break;
            };
            if element.object_type_id != STRING_PROPERTY_ATOM_TYPE || element.object_base_type != 5
            {
                return Ok(None);
            }
            let Some(value) = parse_jt_string_property_atom_body(ctx, element.body)? else {
                return Ok(None);
            };
            let operation = "store DisplayJT string property atom";
            ctx.charge_entities(1, operation)?;
            ctx.push_vec(
                &mut atoms,
                DisplayJtStringPropertyAtom {
                    id: ctx.format_retained(
                        format_args!("{}-string-property-atom-{ordinal}", segment.id),
                        operation,
                    )?,
                    element: ctx.format_retained(
                        format_args!("{}-inflated-element-{ordinal}", segment.id),
                        operation,
                    )?,
                    object_id: element.object_id,
                    value,
                    source_offset: segment.source_offset + 24,
                },
                operation,
            )?;
        }
    }
    Ok(Some(atoms))
    })?;
    let Some(atoms) = candidate else {
        return Ok(Vec::new());
    };
    if !atoms.is_empty() {
        candidate_storage.commit()?;
    }
    Ok(atoms)
}

/// Fields of one late-loaded property atom in a scene property table.
type JtLateLoadedProperty = (u32, u16, [u8; 16], u32, u32, u32);

/// Resolve JT 9 logical shape nodes to their late-loaded type-7 LOD segments.
pub(super) fn display_jt_shape_lod_bindings(
    ctx: &DecodeContext<'_>,
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
    let (indexed_targets, targets_storage) = ctx.unique_index(
        segments.iter().map(|segment| {
            (
                (
                    segment.document.as_str(),
                    segment.segment_id,
                    segment.segment_type,
                ),
                segment,
            )
        }),
        "index DisplayJT binding targets",
    )?;
    let _targets_storage = targets_storage;
    let targets = indexed_targets;
    let mut candidate_storage = ctx.reserve_scoped(0, "nx JT shape LOD binding candidates")?;
    let candidate = candidate_storage.with_storage(|| -> Result<
        Option<Vec<DisplayJtShapeLodBinding>>,
        CodecError,
    > {
    let mut bindings = Vec::new();
    let mut remaining_segments = segments.iter();
    while !remaining_segments.as_slice().is_empty() {
        let Some(scene_segment) =
            ctx.next_charged(&mut remaining_segments, "scan DisplayJT binding segments")?
        else {
            break;
        };
        if scene_segment.segment_type != 1 {
            continue;
        }
        let Some((inflated_candidate, inflated_storage)) =
            inflate_display_jt_segment(ctx, container, scene_segment)?
        else {
            return Ok(None);
        };
        let _inflated_storage = inflated_storage;
        let inflated = inflated_candidate;
        let Some((scene_elements, scene_end, scene_framing_storage)) =
            frame_display_jt_elements(ctx, &inflated, "NX JT framing storage")?
        else {
            return Ok(None);
        };
        drop(scene_elements);
        drop(scene_framing_storage);
        let tail = &inflated[scene_end..];
        let Some((property_atoms_candidate, property_table_offset, property_framing_storage)) =
            frame_display_jt_elements(ctx, tail, "NX JT property framing storage")?
        else {
            return Ok(None);
        };
        let _property_framing_storage = property_framing_storage;
        let property_atoms = property_atoms_candidate;
        let mut property_storage = ctx.reserve_scoped(0, "store DisplayJT property atoms")?;
        let mut shape_keys = HashMap::new();
        let mut late_loaded = HashMap::<u32, JtLateLoadedProperty>::new();
        let mut remaining_atoms = property_atoms.iter();
        while !remaining_atoms.as_slice().is_empty() {
            let Some(atom) =
                ctx.next_charged(&mut remaining_atoms, "scan DisplayJT property atoms")?
            else {
                break;
            };
            if atom.object_type_id == STRING_PROPERTY_ATOM_TYPE && atom.object_base_type == 5 {
                let mut value_storage = ctx.reserve_scoped(0, "DisplayJT binding property string")?;
                let Some(value) = value_storage
                    .with_storage(|| parse_jt_string_property_atom_body(ctx, atom.body))?
                else {
                    return Ok(None);
                };
                let shape_key = value == SHAPE_IMPLEMENTATION_KEY;
                drop(value);
                drop(value_storage);
                property_storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut shape_keys,
                        atom.object_id,
                        shape_key,
                        "store DisplayJT property atoms",
                    )
                })?;
            } else if atom.object_type_id == LATE_LOADED_PROPERTY_ATOM_TYPE
                && atom.object_base_type == 8
            {
                if atom.body.len() != 36 || View::u16_le_at(atom.body, 0) != Some(1) {
                    return Ok(None);
                }
                let Some(state_flags) = View::u32_le_at(atom.body, 2) else {
                    return Ok(None);
                };
                let Some(property_version) = View::u16_le_at(atom.body, 6) else {
                    return Ok(None);
                };
                let Ok(segment_id) = <[u8; 16]>::try_from(&atom.body[8..24]) else {
                    return Ok(None);
                };
                let Some(segment_type) = View::u32_le_at(atom.body, 24) else {
                    return Ok(None);
                };
                let Some(payload_object_id) = View::u32_le_at(atom.body, 28) else {
                    return Ok(None);
                };
                let Some(reserved_value) =
                    View::u32_le_at(atom.body, 32).filter(|value| *value != 0)
                else {
                    return Ok(None);
                };
                property_storage.with_storage(|| {
                    ctx.insert_hash_map(
                        &mut late_loaded,
                        atom.object_id,
                        (
                            state_flags,
                            property_version,
                            segment_id,
                            segment_type,
                            payload_object_id,
                            reserved_value,
                        ),
                        "store DisplayJT property atoms",
                    )
                })?;
            }
        }
        let table = &tail[property_table_offset..];
        let mut table_view = View::over_retained(table);
        let Some(table_version) = table_view.u16_le() else {
            return Ok(None);
        };
        // Each table entry holds a shape-node identifier and a zero key terminator.
        let Some(table_count) = table_view
            .u32_le()
            .and_then(|count| table_view.counted(u64::from(count), 8))
        else {
            return Ok(None);
        };
        let mut table_ordinals = 0..table_count.get();
        while table_ordinals.len() != 0 {
            let Some(table_ordinal) =
                ctx.next_charged(&mut table_ordinals, "scan DisplayJT shape LOD property tables")?
            else {
                break;
            };
            let Some(shape_node_object_id) = table_view.u32_le() else {
                return Ok(None);
            };
            let mut pair_ordinal = 0u32;
            loop {
                ctx.charge_work(1, "scan DisplayJT shape LOD property pairs")?;
                let Some(key_object_id) = table_view.u32_le() else {
                    return Ok(None);
                };
                if key_object_id == 0 {
                    break;
                }
                let Some(value_object_id) = table_view.u32_le() else {
                    return Ok(None);
                };
                let shape_key = ctx
                    .get_hash_map(&shape_keys, &key_object_id, "match DisplayJT property keys")?
                    .copied()
                    .unwrap_or(false);
                if shape_key {
                    let Some(&(
                        state_flags,
                        property_version,
                        segment_id,
                        segment_type,
                        payload_object_id,
                        reserved_value,
                    )) = ctx.get_hash_map(
                        &late_loaded,
                        &value_object_id,
                        "match DisplayJT late-loaded properties",
                    )?
                    else {
                        return Ok(None);
                    };
                    let Some(&Some(target)) = ctx.get_hash_map(
                        &targets,
                        &(scene_segment.document.as_str(), segment_id, segment_type),
                        "match DisplayJT binding targets",
                    )?
                    else {
                        return Ok(None);
                    };
                    if target.segment_type != 7 {
                        return Ok(None);
                    }
                    let operation = "store DisplayJT shape LOD binding";
                    ctx.charge_entities(1, operation)?;
                    ctx.push_vec(
                        &mut bindings,
                        DisplayJtShapeLodBinding {
                            id: ctx.format_retained(
                                format_args!(
                                    "{}-shape-lod-binding-{table_ordinal}-{pair_ordinal}",
                                    scene_segment.id
                                ),
                                operation,
                            )?,
                            scene_segment: ctx.copy_retained_text(&scene_segment.id, operation)?,
                            table_version,
                            shape_node_object_id,
                            key_object_id,
                            key: ctx.copy_retained_text(SHAPE_IMPLEMENTATION_KEY, operation)?,
                            value_object_id,
                            state_flags,
                            property_version,
                            shape_segment: ctx.copy_retained_text(&target.id, operation)?,
                            payload_object_id,
                            reserved_value,
                            source_offset: scene_segment.source_offset + 24,
                        },
                        operation,
                    )?;
                }
                pair_ordinal += 1;
            }
        }
        if !table_view.is_empty() {
            return Ok(None);
        }
    }
    Ok(Some(bindings))
    })?;
    let Some(bindings) = candidate else {
        return Ok(Vec::new());
    };
    if !bindings.is_empty() {
        candidate_storage.commit()?;
    }
    Ok(bindings)
}

/// Scene-graph node and attribute records of the logical scene-graph segments.
///
/// Each family is decoded independently: a family that rejects a segment or
/// element decodes no records, and the other families are unaffected.
pub(super) struct DisplayJtSceneNodes {
    pub(super) base_nodes: Vec<DisplayJtBaseNodeData>,
    pub(super) group_nodes: Vec<DisplayJtGroupNodeData>,
    pub(super) instance_nodes: Vec<DisplayJtInstanceNode>,
    pub(super) transforms: Vec<DisplayJtGeometricTransformAttribute>,
    pub(super) materials: Vec<DisplayJtMaterialAttribute>,
    pub(super) partition_nodes: Vec<DisplayJtPartitionNode>,
    pub(super) range_lod_nodes: Vec<DisplayJtRangeLodNode>,
    pub(super) tri_strip_shape_nodes: Vec<DisplayJtTriStripShapeNode>,
}

/// Candidate records and the storage they need if the family is accepted.
struct JtSceneFamily<'ctx, T> {
    candidate: Option<(Vec<T>, ScopedReservation<'ctx>)>,
}

impl<'ctx, T> JtSceneFamily<'ctx, T> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            candidate: Some((Vec::new(), ctx.reserve_scoped(0, operation)?)),
        })
    }

    fn is_active(&self) -> bool {
        self.candidate.is_some()
    }

    fn as_mut(&mut self) -> Option<(&mut Vec<T>, &mut ScopedReservation<'ctx>)> {
        self.candidate
            .as_mut()
            .map(|(records, storage)| (records, storage))
    }

    fn reject(&mut self) {
        self.candidate = None;
    }

    fn finish(self) -> Result<Vec<T>, CodecError> {
        let Some((candidate_records, storage)) = self.candidate else {
            return Ok(Vec::new());
        };
        let records = candidate_records;
        if !records.is_empty() {
            storage.commit()?;
        }
        Ok(records)
    }
}

/// Rejects a family whose segment admission applies to the current segment.
fn reject_jt_scene_family<T>(family: &mut JtSceneFamily<'_, T>, applies: bool) {
    if applies {
        family.reject();
    }
}

/// Decode every scene-graph node family from one inflation of each type-1 segment.
///
/// Common node data comes from every compressed segment; the JT 9 node,
/// attribute and shape families from compressed segments of JT 9 documents;
/// partition and range-LOD nodes from segments of documents before JT 10.
pub(super) fn display_jt_scene_nodes(
    ctx: &DecodeContext<'_>,
    container: &Container,
    segments: &[DisplayJtSegment],
    documents: &[DisplayJtDocument],
) -> Result<DisplayJtSceneNodes, CodecError> {
    let (documents_by_id_candidate, documents_storage) = ctx.unique_index(
        documents
            .iter()
            .map(|document| (document.id.as_str(), document)),
        "index DisplayJT documents",
    )?;
    let _documents_storage = documents_storage;
    let documents_by_id = documents_by_id_candidate;
    let mut output_storage = ctx.reserve_scoped(0, "nx JT scene node candidates")?;
    let output = output_storage.with_storage(|| -> Result<DisplayJtSceneNodes, CodecError> {
    let mut base_nodes = JtSceneFamily::new(ctx, "nx JT base node candidates")?;
    let mut group_nodes = JtSceneFamily::new(ctx, "nx JT group node candidates")?;
    let mut instance_nodes = JtSceneFamily::new(ctx, "nx JT instance node candidates")?;
    let mut transforms = JtSceneFamily::new(ctx, "nx JT transform candidates")?;
    let mut materials = JtSceneFamily::new(ctx, "nx JT material candidates")?;
    let mut partition_nodes = JtSceneFamily::new(ctx, "nx JT partition node candidates")?;
    let mut range_lod_nodes = JtSceneFamily::new(ctx, "nx JT range LOD candidates")?;
    let mut tri_strip_shape_nodes = JtSceneFamily::new(ctx, "nx JT tri-strip shape candidates")?;
    let mut remaining_segments = segments.iter();
    while !remaining_segments.as_slice().is_empty() {
        let Some(segment) =
            ctx.next_charged(&mut remaining_segments, "scan DisplayJT scene segments")?
        else {
            break;
        };
        if segment.segment_type != 1 {
            continue;
        }
        let Some(&Some(document)) = ctx.get_hash_map(
            &documents_by_id,
            segment.document.as_str(),
            "match DisplayJT segment documents",
        )?
        else {
            return Ok(DisplayJtSceneNodes {
                base_nodes: Vec::new(),
                group_nodes: Vec::new(),
                instance_nodes: Vec::new(),
                transforms: Vec::new(),
                materials: Vec::new(),
                partition_nodes: Vec::new(),
                range_lod_nodes: Vec::new(),
                tri_strip_shape_nodes: Vec::new(),
            });
        };
        let compressed = segment.compression.is_some();
        reject_jt_scene_family(&mut base_nodes, !compressed);
        let jt9 = document.version.major() == 9 && compressed;
        let legacy = document.version.major() < 10;
        let jt9_active = jt9
            && (group_nodes.is_active()
                || instance_nodes.is_active()
                || transforms.is_active()
                || materials.is_active()
                || tri_strip_shape_nodes.is_active());
        let legacy_active = legacy
            && (partition_nodes.is_active() || range_lod_nodes.is_active());
        if !base_nodes.is_active() && !jt9_active && !legacy_active {
            continue;
        }
        let inflated = inflate_display_jt_segment(ctx, container, segment)?;
        let framed_elements = match &inflated {
            Some((bytes, _)) => frame_display_jt_elements(ctx, bytes, "NX JT framing storage")?,
            None => None,
        };
        let Some((elements_candidate, _, framing_storage)) = framed_elements else {
            reject_jt_scene_family(&mut base_nodes, true);
            reject_jt_scene_family(&mut group_nodes, jt9);
            reject_jt_scene_family(&mut instance_nodes, jt9);
            reject_jt_scene_family(&mut transforms, jt9);
            reject_jt_scene_family(&mut materials, jt9);
            reject_jt_scene_family(&mut tri_strip_shape_nodes, jt9);
            reject_jt_scene_family(&mut partition_nodes, legacy);
            reject_jt_scene_family(&mut range_lod_nodes, legacy);
            continue;
        };
        let _framing_storage = framing_storage;
        let elements = elements_candidate;
        let mut remaining_elements = elements.iter().enumerate();
        while remaining_elements.len() != 0 {
            let Some((ordinal, element)) =
                ctx.next_charged(&mut remaining_elements, "decode DisplayJT scene elements")?
            else {
                break;
            };
            let at = JtSceneElement {
                segment,
                ordinal,
                element,
            };
            if let Some((nodes, storage)) = base_nodes.as_mut() {
                if !storage.with_storage(|| {
                    push_jt_base_node(ctx, nodes, &at, document.version.major())
                })? {
                    base_nodes.reject();
                }
            }
            if jt9 {
                if let Some((nodes, storage)) = group_nodes.as_mut() {
                    if !storage.with_storage(|| push_jt_group_node(ctx, nodes, &at))? {
                        group_nodes.reject();
                    }
                }
                if let Some((nodes, storage)) = instance_nodes.as_mut() {
                    if !storage.with_storage(|| push_jt_instance_node(ctx, nodes, &at))? {
                        instance_nodes.reject();
                    }
                }
                if let Some((attributes, storage)) = transforms.as_mut() {
                    if !storage.with_storage(|| push_jt_geometric_transform(ctx, attributes, &at))? {
                        transforms.reject();
                    }
                }
                if let Some((attributes, storage)) = materials.as_mut() {
                    if !storage.with_storage(|| push_jt_material(ctx, attributes, &at))? {
                        materials.reject();
                    }
                }
                if let Some((nodes, storage)) = tri_strip_shape_nodes.as_mut() {
                    if !storage.with_storage(|| push_jt_tri_strip_shape_node(ctx, nodes, &at))? {
                        tri_strip_shape_nodes.reject();
                    }
                }
            }
            if legacy {
                if let Some((nodes, storage)) = partition_nodes.as_mut() {
                    if !storage.with_storage(|| push_jt_partition_node(ctx, nodes, &at))? {
                        partition_nodes.reject();
                    }
                }
                if let Some((nodes, storage)) = range_lod_nodes.as_mut() {
                    if !storage.with_storage(|| push_jt_range_lod_node(ctx, nodes, &at))? {
                        range_lod_nodes.reject();
                    }
                }
            }
        }
    }
    Ok(DisplayJtSceneNodes {
        base_nodes: base_nodes.finish()?,
        group_nodes: group_nodes.finish()?,
        instance_nodes: instance_nodes.finish()?,
        transforms: transforms.finish()?,
        materials: materials.finish()?,
        partition_nodes: partition_nodes.finish()?,
        range_lod_nodes: range_lod_nodes.finish()?,
        tri_strip_shape_nodes: tri_strip_shape_nodes.finish()?,
    })
    })?;
    if !output.base_nodes.is_empty()
        || !output.group_nodes.is_empty()
        || !output.instance_nodes.is_empty()
        || !output.transforms.is_empty()
        || !output.materials.is_empty()
        || !output.partition_nodes.is_empty()
        || !output.range_lod_nodes.is_empty()
        || !output.tri_strip_shape_nodes.is_empty()
    {
        output_storage.commit()?;
    }
    Ok(output)
}

/// One framed element of a scene segment at its serialized ordinal.
struct JtSceneElement<'a, 'b> {
    segment: &'a DisplayJtSegment,
    ordinal: usize,
    element: &'a ParsedJtElement<'b>,
}

/// Decodes the common node data of a node element; `false` rejects the family.
fn push_jt_base_node(
    ctx: &DecodeContext<'_>,
    nodes: &mut Vec<DisplayJtBaseNodeData>,
    at: &JtSceneElement<'_, '_>,
    format_major: u16,
) -> Result<bool, CodecError> {
    let JtSceneElement {
        segment,
        ordinal,
        element,
    } = *at;
    if element.object_base_type > 2 {
        return Ok(true);
    }
    let Some((version, flags, attribute_object_ids, family_data)) =
        parse_jt_base_node_body(element.body, format_major)
    else {
        return Ok(false);
    };
    let attribute_object_ids = read_jt_object_ids(
        ctx,
        attribute_object_ids,
        "decode DisplayJT base node attributes",
    )?;
    let operation = "store DisplayJT base node";
    ctx.charge_entities(1, operation)?;
    ctx.push_vec(
        nodes,
        DisplayJtBaseNodeData {
            id: ctx.format_retained(
                format_args!("{}-base-node-{ordinal}", segment.id),
                operation,
            )?,
            element: ctx.format_retained(
                format_args!("{}-inflated-element-{ordinal}", segment.id),
                operation,
            )?,
            object_type_id: element.object_type_id,
            object_id: element.object_id,
            version,
            flags,
            attribute_object_ids,
            family_data_byte_len: u32::try_from(family_data.len()).map_err(|_| {
                ctx.refuse_codec_limit(
                    "DisplayJT count exceeds u32",
                    u64::from(u32::MAX),
                    cadmpeg_core::decode::u64_from_index(family_data.len()),
                )
            })?,
            family_data_sha256: Sha256Digest::digest_for_decode(
                ctx,
                family_data,
                "retain DisplayJT hash",
            )?,
            source_offset: segment.source_offset + 24,
        },
        operation,
    )?;
    Ok(true)
}

/// Decodes JT 9 group-node data of a group element; `false` rejects the family.
fn push_jt_group_node(
    ctx: &DecodeContext<'_>,
    nodes: &mut Vec<DisplayJtGroupNodeData>,
    at: &JtSceneElement<'_, '_>,
) -> Result<bool, CodecError> {
    let JtSceneElement {
        segment,
        ordinal,
        element,
    } = *at;
    if element.object_base_type != 1 {
        return Ok(true);
    }
    let Some((version, child_object_ids, family_data)) = parse_jt9_group_node_body(element.body)
    else {
        return Ok(false);
    };
    let child_object_ids =
        read_jt_object_ids(ctx, child_object_ids, "decode DisplayJT group children")?;
    if version != 1 {
        return Ok(false);
    }
    let operation = "store DisplayJT group node";
    ctx.charge_entities(1, operation)?;
    ctx.push_vec(
        nodes,
        DisplayJtGroupNodeData {
            id: ctx.format_retained(
                format_args!("{}-group-node-data-{ordinal}", segment.id),
                operation,
            )?,
            base_node: ctx.format_retained(
                format_args!("{}-base-node-{ordinal}", segment.id),
                operation,
            )?,
            object_id: element.object_id,
            version,
            child_object_ids,
            family_data_byte_len: u32::try_from(family_data.len()).map_err(|_| {
                ctx.refuse_codec_limit(
                    "DisplayJT count exceeds u32",
                    u64::from(u32::MAX),
                    cadmpeg_core::decode::u64_from_index(family_data.len()),
                )
            })?,
            family_data_sha256: Sha256Digest::digest_for_decode(
                ctx,
                family_data,
                "retain DisplayJT hash",
            )?,
            source_offset: segment.source_offset + 24,
        },
        operation,
    )?;
    Ok(true)
}

/// Decodes a JT 9 instance node; `false` rejects the family.
fn push_jt_instance_node(
    ctx: &DecodeContext<'_>,
    nodes: &mut Vec<DisplayJtInstanceNode>,
    at: &JtSceneElement<'_, '_>,
) -> Result<bool, CodecError> {
    const INSTANCE_NODE_TYPE: [u8; 16] = [
        0x2a, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let JtSceneElement {
        segment,
        ordinal,
        element,
    } = *at;
    if element.object_type_id != INSTANCE_NODE_TYPE {
        return Ok(true);
    }
    if element.object_base_type != 0 {
        return Ok(false);
    }
    let Some((version, child_object_id)) = parse_jt9_instance_node_body(element.body) else {
        return Ok(false);
    };
    let operation = "store DisplayJT instance node";
    ctx.charge_entities(1, operation)?;
    ctx.push_vec(
        nodes,
        DisplayJtInstanceNode {
            id: ctx.format_retained(
                format_args!("{}-instance-node-{ordinal}", segment.id),
                operation,
            )?,
            base_node: ctx.format_retained(
                format_args!("{}-base-node-{ordinal}", segment.id),
                operation,
            )?,
            object_id: element.object_id,
            version,
            child_object_id,
            source_offset: segment.source_offset + 24,
        },
        operation,
    )?;
    Ok(true)
}

/// Decodes a JT 9 geometric-transform attribute; `false` rejects the family.
fn push_jt_geometric_transform(
    ctx: &DecodeContext<'_>,
    attributes: &mut Vec<DisplayJtGeometricTransformAttribute>,
    at: &JtSceneElement<'_, '_>,
) -> Result<bool, CodecError> {
    const GEOMETRIC_TRANSFORM_TYPE: [u8; 16] = [
        0x83, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let JtSceneElement {
        segment,
        ordinal,
        element,
    } = *at;
    if element.object_type_id != GEOMETRIC_TRANSFORM_TYPE {
        return Ok(true);
    }
    if element.object_base_type != 3 {
        return Ok(false);
    }
    let Some((state_flags, field_inhibit_flags, stored_values_mask, matrix)) =
        parse_jt9_geometric_transform_body(element.body)
    else {
        return Ok(false);
    };
    let operation = "store DisplayJT geometric transform";
    ctx.charge_entities(1, operation)?;
    ctx.push_vec(
        attributes,
        DisplayJtGeometricTransformAttribute {
            id: ctx.format_retained(
                format_args!("{}-geometric-transform-{ordinal}", segment.id),
                operation,
            )?,
            element: ctx.format_retained(
                format_args!("{}-inflated-element-{ordinal}", segment.id),
                operation,
            )?,
            object_id: element.object_id,
            state_flags,
            field_inhibit_flags,
            stored_values_mask,
            matrix,
            source_offset: segment.source_offset + 24,
        },
        operation,
    )?;
    Ok(true)
}

/// Decodes a JT 9 material attribute; `false` rejects the family.
fn push_jt_material(
    ctx: &DecodeContext<'_>,
    attributes: &mut Vec<DisplayJtMaterialAttribute>,
    at: &JtSceneElement<'_, '_>,
) -> Result<bool, CodecError> {
    const MATERIAL_ATTRIBUTE_TYPE: [u8; 16] = [
        0x30, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let JtSceneElement {
        segment,
        ordinal,
        element,
    } = *at;
    if element.object_type_id != MATERIAL_ATTRIBUTE_TYPE {
        return Ok(true);
    }
    if element.object_base_type != 3 {
        return Ok(false);
    }
    let Some((state_flags, field_inhibit_flags, version, data_flags, colors, shininess)) =
        parse_jt9_material_body(element.body)
    else {
        return Ok(false);
    };
    let operation = "store DisplayJT material attribute";
    ctx.charge_entities(1, operation)?;
    ctx.push_vec(
        attributes,
        DisplayJtMaterialAttribute {
            id: ctx.format_retained(
                format_args!("{}-material-attribute-{ordinal}", segment.id),
                operation,
            )?,
            element: ctx.format_retained(
                format_args!("{}-inflated-element-{ordinal}", segment.id),
                operation,
            )?,
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
        },
        operation,
    )?;
    Ok(true)
}

/// Decodes a partition node before JT 10; `false` rejects the family.
fn push_jt_partition_node(
    ctx: &DecodeContext<'_>,
    nodes: &mut Vec<DisplayJtPartitionNode>,
    at: &JtSceneElement<'_, '_>,
) -> Result<bool, CodecError> {
    const PARTITION_NODE_TYPE: [u8; 16] = [
        0x3e, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let JtSceneElement {
        segment,
        ordinal,
        element,
    } = *at;
    if element.object_type_id != PARTITION_NODE_TYPE {
        return Ok(true);
    }
    let Some(node) = parse_jt9_partition_node_body(ctx, element.body)? else {
        return Ok(false);
    };
    let operation = "store DisplayJT partition node";
    ctx.charge_entities(1, operation)?;
    ctx.push_vec(
        nodes,
        DisplayJtPartitionNode {
            id: ctx.format_retained(
                format_args!("{}-partition-node-{ordinal}", segment.id),
                operation,
            )?,
            base_node: ctx.format_retained(
                format_args!("{}-base-node-{ordinal}", segment.id),
                operation,
            )?,
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
        },
        operation,
    )?;
    Ok(true)
}

/// Decodes a range-LOD node before JT 10; `false` rejects the family.
fn push_jt_range_lod_node(
    ctx: &DecodeContext<'_>,
    nodes: &mut Vec<DisplayJtRangeLodNode>,
    at: &JtSceneElement<'_, '_>,
) -> Result<bool, CodecError> {
    const RANGE_LOD_NODE_TYPE: [u8; 16] = [
        0x4c, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let JtSceneElement {
        segment,
        ordinal,
        element,
    } = *at;
    if element.object_type_id != RANGE_LOD_NODE_TYPE {
        return Ok(true);
    }
    let Some(node) = parse_jt9_range_lod_node_body(ctx, element.body)? else {
        return Ok(false);
    };
    let operation = "store DisplayJT range LOD node";
    ctx.charge_entities(1, operation)?;
    ctx.push_vec(
        nodes,
        DisplayJtRangeLodNode {
            id: ctx.format_retained(
                format_args!("{}-range-lod-node-{ordinal}", segment.id),
                operation,
            )?,
            base_node: ctx.format_retained(
                format_args!("{}-base-node-{ordinal}", segment.id),
                operation,
            )?,
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
        },
        operation,
    )?;
    Ok(true)
}

/// Decodes a JT 9 tri-strip shape node; `false` rejects the family.
fn push_jt_tri_strip_shape_node(
    ctx: &DecodeContext<'_>,
    nodes: &mut Vec<DisplayJtTriStripShapeNode>,
    at: &JtSceneElement<'_, '_>,
) -> Result<bool, CodecError> {
    const TRI_STRIP_SHAPE_NODE_TYPE: [u8; 16] = [
        0x77, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59,
        0x97,
    ];
    let JtSceneElement {
        segment,
        ordinal,
        element,
    } = *at;
    if element.object_type_id != TRI_STRIP_SHAPE_NODE_TYPE {
        return Ok(true);
    }
    if element.object_base_type != 2 {
        return Ok(false);
    }
    let Some(node) = parse_jt9_tri_strip_shape_node_body(element.body) else {
        return Ok(false);
    };
    let operation = "store DisplayJT tri strip shape node";
    ctx.charge_entities(1, operation)?;
    ctx.push_vec(
        nodes,
        DisplayJtTriStripShapeNode {
            id: ctx.format_retained(
                format_args!("{}-tri-strip-shape-node-{ordinal}", segment.id),
                operation,
            )?,
            base_node: ctx.format_retained(
                format_args!("{}-base-node-{ordinal}", segment.id),
                operation,
            )?,
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
        },
        operation,
    )?;
    Ok(true)
}
const DISPLAY_JT_COLOR_CHANNEL: u32 = 0x4e58_0001;
const DISPLAY_JT_VERTEX_FLAG_CHANNEL: u32 = 0x4e58_0002;
const DISPLAY_JT_TEXTURE_CHANNEL_BASE: u32 = 0x4e58_0100;
type DisplayJtMatrix = [[f64; 4]; 4];

struct DisplayJtPath<'ctx> {
    matrix: DisplayJtMatrix,
    final_transform: bool,
    diffuse: [Option<UnitBinary32>; 4],
    override_vertex_colors: Option<bool>,
    final_material: bool,
    node_path: Vec<u32>,
    instance_path: Vec<String>,
    node_storage: ScopedReservation<'ctx>,
    instance_storage: ScopedReservation<'ctx>,
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
    path: &mut DisplayJtPath<'_>,
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

fn display_jt_path_color(path: &DisplayJtPath<'_>) -> Option<Color> {
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

/// Path states own their child buffers; this receipt owns only vector slots.
struct JtResolvedPaths<'ctx> {
    values: Vec<DisplayJtPath<'ctx>>,
    storage: ScopedReservation<'ctx>,
}

/// A keyed record, or a key that more than one record carries.
#[derive(Clone, Copy)]
enum JtKeyed<T> {
    Unique(T),
    Repeated,
}

/// Records a keyed value, turning a second occurrence of its key into a repeat.
fn insert_jt_keyed<K: Eq + std::hash::Hash + cadmpeg_core::decode::cost::DecodeCost, V>(
    ctx: &DecodeContext<'_>,
    storage: &mut ScopedReservation<'_>,
    table: &mut HashMap<K, JtKeyed<V>>,
    key: K,
    value: V,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(previous) = ctx.get_mut_hash_map(table, &key, operation)? {
        *previous = JtKeyed::Repeated;
        return Ok(());
    }
    storage.with_storage(|| ctx.insert_hash_map(table, key, JtKeyed::Unique(value), operation))?;
    Ok(())
}

/// Scene-independent lookup tables over the tessellation inputs.
struct JtTessellationIndex<'a, 'ctx> {
    element_segments: HashMap<&'a str, Option<&'a str>>,
    base_nodes: HashMap<&'a str, Option<&'a DisplayJtBaseNodeData>>,
    group_children: HashMap<&'a str, JtKeyed<&'a [u32]>>,
    instance_children: HashMap<&'a str, JtKeyed<&'a [u32]>>,
    _storage: [ScopedReservation<'ctx>; 4],
}

impl<'a, 'ctx> JtTessellationIndex<'a, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        inputs: &DisplayJtTessellationInputs<'a>,
    ) -> Result<Self, CodecError> {
        let (element_segments_candidate, element_storage_candidate) = ctx.unique_index(
            inputs
                .compressed_elements
                .iter()
                .map(|element| (element.id.as_str(), element.segment.as_str())),
            "index JT scene elements",
        )?;
        let element_storage = element_storage_candidate;
        let element_segments = element_segments_candidate;
        let (base_nodes_candidate, base_storage_candidate) = ctx.unique_index(
            inputs
                .base_nodes
                .iter()
                .map(|base| (base.id.as_str(), base)),
            "index JT base nodes",
        )?;
        let base_storage = base_storage_candidate;
        let base_nodes = base_nodes_candidate;
        let mut group_storage = ctx.reserve_scoped(0, "index JT group children")?;
        let mut group_children = HashMap::new();
        let mut remaining_group_nodes = inputs.group_nodes.iter();
        while !remaining_group_nodes.as_slice().is_empty() {
            let Some(node) =
                ctx.next_charged(&mut remaining_group_nodes, "index JT group children")?
            else {
                break;
            };
            insert_jt_keyed(
                ctx,
                &mut group_storage,
                &mut group_children,
                node.base_node.as_str(),
                node.child_object_ids.as_slice(),
                "index JT group children",
            )?;
        }
        let mut instance_storage = ctx.reserve_scoped(0, "index JT instance children")?;
        let mut instance_children = HashMap::new();
        let mut remaining_instance_nodes = inputs.instance_nodes.iter();
        while !remaining_instance_nodes.as_slice().is_empty() {
            let Some(node) = ctx
                .next_charged(&mut remaining_instance_nodes, "index JT instance children")?
            else {
                break;
            };
            insert_jt_keyed(
                ctx,
                &mut instance_storage,
                &mut instance_children,
                node.base_node.as_str(),
                std::slice::from_ref(&node.child_object_id),
                "index JT instance children",
            )?;
        }
        Ok(Self {
            element_segments,
            base_nodes,
            group_children,
            instance_children,
            _storage: [
                element_storage,
                base_storage,
                group_storage,
                instance_storage,
            ],
        })
    }

    /// The scene segment that owns a compressed element.
    fn element_segment(
        &self,
        ctx: &DecodeContext<'_>,
        element: &str,
    ) -> Result<Option<&'a str>, CodecError> {
        Ok(ctx
            .get_hash_map(&self.element_segments, element, "match JT scene elements")?
            .copied()
            .flatten())
    }

    fn base_node(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<Option<&'a DisplayJtBaseNodeData>, CodecError> {
        Ok(ctx
            .get_hash_map(&self.base_nodes, id, "match JT base nodes")?
            .copied()
            .flatten())
    }

    /// Whether a compressed element belongs to a scene segment.
    fn in_scene(
        &self,
        ctx: &DecodeContext<'_>,
        element: &str,
        scene_segment: &str,
    ) -> Result<bool, CodecError> {
        match self.element_segment(ctx, element)? {
            Some(segment) => ctx.equal(segment, scene_segment, "match JT scene segments"),
            None => Ok(false),
        }
    }
}

/// The node graph of one logical scene segment.
struct JtSceneGraph<'a, 'ctx> {
    by_object: BTreeMap<u32, &'a DisplayJtBaseNodeData>,
    parents: HashMap<u32, Vec<u32>>,
    instance_ids: HashMap<u32, &'a str>,
    transforms: HashMap<u32, JtKeyed<&'a DisplayJtGeometricTransformAttribute>>,
    materials: HashMap<u32, JtKeyed<&'a DisplayJtMaterialAttribute>>,
    _storage: ScopedReservation<'ctx>,
}

impl<'a, 'ctx> JtSceneGraph<'a, 'ctx> {
    /// Builds the scene graph, or `None` when the scene does not form one.
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        scene_segment: &str,
        inputs: &DisplayJtTessellationInputs<'a>,
        index: &JtTessellationIndex<'a, '_>,
    ) -> Result<Option<Self>, CodecError> {
        let mut storage = ctx.reserve_scoped(0, "nx JT scene graph")?;
        let mut by_object = BTreeMap::new();
        let mut remaining_bases = inputs.base_nodes.iter();
        while !remaining_bases.as_slice().is_empty() {
            let Some(base) = ctx.next_charged(&mut remaining_bases, "nx JT scoped base nodes")?
            else {
                break;
            };
            if !index.in_scene(ctx, &base.element, scene_segment)? {
                continue;
            }
            if ctx.contains_key_btree_map(&by_object, &base.object_id, "nx JT node index")? {
                return Ok(None);
            }
            storage.with_storage(|| {
                ctx.insert_btree_map(&mut by_object, base.object_id, base, "nx JT node index")
            })?;
        }
        let mut transforms = HashMap::new();
        let mut remaining_transforms = inputs.transforms.iter();
        while !remaining_transforms.as_slice().is_empty() {
            let Some(attribute) =
                ctx.next_charged(&mut remaining_transforms, "nx JT scoped transforms")?
            else {
                break;
            };
            if index.in_scene(ctx, &attribute.element, scene_segment)? {
                insert_jt_keyed(
                    ctx,
                    &mut storage,
                    &mut transforms,
                    attribute.object_id,
                    attribute,
                    "nx JT scoped transforms",
                )?;
            }
        }
        let mut materials = HashMap::new();
        let mut remaining_materials = inputs.materials.iter();
        while !remaining_materials.as_slice().is_empty() {
            let Some(attribute) =
                ctx.next_charged(&mut remaining_materials, "nx JT scoped materials")?
            else {
                break;
            };
            if index.in_scene(ctx, &attribute.element, scene_segment)? {
                insert_jt_keyed(
                    ctx,
                    &mut storage,
                    &mut materials,
                    attribute.object_id,
                    attribute,
                    "nx JT scoped materials",
                )?;
            }
        }
        let mut instance_ids = HashMap::new();
        let mut remaining_instances = inputs.instance_nodes.iter();
        while !remaining_instances.as_slice().is_empty() {
            let Some(node) = ctx.next_charged(&mut remaining_instances, "nx JT instance index")?
            else {
                break;
            };
            let Some(base) = index.base_node(ctx, &node.base_node)? else {
                continue;
            };
            if !index.in_scene(ctx, &base.element, scene_segment)? {
                continue;
            }
            if ctx.contains_key_hash_map(&instance_ids, &node.object_id, "nx JT instance index")? {
                return Ok(None);
            }
            storage.with_storage(|| {
                ctx.insert_hash_map(
                    &mut instance_ids,
                    node.object_id,
                    node.id.as_str(),
                    "nx JT instance index",
                )
            })?;
        }
        let mut parents = HashMap::new();
        let mut remaining_base_nodes = by_object.iter();
        while remaining_base_nodes.len() != 0 {
            let Some((&object_id, base)) =
                ctx.next_charged(&mut remaining_base_nodes, "nx JT parent index")?
            else {
                break;
            };
            let group = ctx.get_hash_map(
                &index.group_children,
                base.id.as_str(),
                "nx JT parent index",
            )?;
            let instance = ctx.get_hash_map(
                &index.instance_children,
                base.id.as_str(),
                "nx JT parent index",
            )?;
            let children = match (group, instance) {
                (Some(JtKeyed::Repeated), _)
                | (_, Some(JtKeyed::Repeated))
                | (Some(_), Some(_)) => return Ok(None),
                (Some(JtKeyed::Unique(children)), None)
                | (None, Some(JtKeyed::Unique(children))) => *children,
                (None, None) => &[],
            };
            let mut remaining_children = children.iter();
            while !remaining_children.as_slice().is_empty() {
                let Some(&child) =
                    ctx.next_charged(&mut remaining_children, "nx JT parent references")?
                else {
                    break;
                };
                if !ctx.contains_key_btree_map(&by_object, &child, "nx JT parent index")? {
                    return Ok(None);
                }
                storage.with_storage(|| {
                    ctx.push_hash_group(
                        &mut parents,
                        child,
                        object_id,
                        "nx JT parent index",
                        "nx JT parent references",
                    )
                })?;
            }
        }
        Ok(Some(Self {
            by_object,
            parents,
            instance_ids,
            transforms,
            materials,
            _storage: storage,
        }))
    }

    /// Resolves every root-to-node path of a scene node with the scoped
    /// reservation that holds the paths.
    fn node_paths<'paths>(
        &self,
        ctx: &'paths DecodeContext<'_>,
        object_id: u32,
    ) -> Result<Option<JtResolvedPaths<'paths>>, CodecError> {
        if !ctx.contains_key_btree_map(&self.by_object, &object_id, "nx JT node index")? {
            return Ok(None);
        }
        let mut visiting_storage = ctx.reserve_scoped(0, "nx JT visiting nodes")?;
        let mut visiting = Vec::new();
        self.resolve(ctx, object_id, &mut visiting, &mut visiting_storage)
    }

    /// Each recursive result owns its slots and each path owns its buffers.
    fn resolve<'paths>(
        &self,
        ctx: &'paths DecodeContext<'_>,
        object_id: u32,
        visiting: &mut Vec<u32>,
        visiting_storage: &mut ScopedReservation<'_>,
    ) -> Result<Option<JtResolvedPaths<'paths>>, CodecError> {
        let _depth = ctx.enter_nested("resolve JT node path")?;
        let Some(&base) = ctx.get_btree_map(&self.by_object, &object_id, "nx JT node index")?
        else {
            return Ok(None);
        };
        let mut paths_storage = ctx.reserve_scoped(0, "nx JT resolved paths")?;
        let mut parent_states = Vec::new();
        if base.flags & 1 != 0 {
            return Ok(Some(JtResolvedPaths { values: parent_states, storage: paths_storage }));
        }
        if ctx.contains(visiting, &object_id, "nx JT visiting nodes")? {
            return Ok(None);
        }
        ctx.push_scoped_vec(visiting_storage, visiting, object_id, "nx JT visiting nodes")?;
        if let Some(ids) = ctx.get_hash_map(&self.parents, &object_id, "nx JT parent index")? {
            let mut remaining_ids = ids.iter();
            while !remaining_ids.as_slice().is_empty() {
                let Some(&id) =
                    ctx.next_charged(&mut remaining_ids, "resolve JT parent paths")?
                else {
                    break;
                };
                let Some(paths) =
                    self.resolve(ctx, id, visiting, visiting_storage)?
                else {
                    return Ok(None);
                };
                let JtResolvedPaths { values, storage } = paths;
                if parent_states.capacity() == 0 {
                    parent_states = values;
                    paths_storage = storage;
                } else {
                    paths_storage.with_storage(|| {
                        ctx.extend_vec(&mut parent_states, values, "nx JT parent path states")
                    })?;
                    drop(storage);
                }
            }
        } else {
            ctx.push_scoped_vec(
                &mut paths_storage,
                &mut parent_states,
                DisplayJtPath {
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
                    node_storage: ctx.reserve_scoped(0, "nx JT node path nodes")?,
                    instance_storage: ctx.reserve_scoped(0, "nx JT instance path nodes")?,
                },
                "nx JT root path state",
            )?;
        }
        visiting.pop();
        let instance_id =
            ctx.get_hash_map(&self.instance_ids, &object_id, "nx JT instance index")?;
        let mut remaining_parent_states = parent_states.iter_mut();
        while remaining_parent_states.len() != 0 {
            let Some(path) =
                ctx.next_charged(&mut remaining_parent_states, "resolve JT path states")?
            else {
                break;
            };
            let mut remaining_attributes = base.attribute_object_ids.iter();
            while !remaining_attributes.as_slice().is_empty() {
                let Some(attribute_id) =
                    ctx.next_charged(&mut remaining_attributes, "resolve JT path attributes")?
                else {
                    break;
                };
                let transform =
                    ctx.get_hash_map(&self.transforms, attribute_id, "resolve JT path attributes")?;
                let material =
                    ctx.get_hash_map(&self.materials, attribute_id, "resolve JT path attributes")?;
                let (transform, material) = match (transform, material) {
                    (Some(JtKeyed::Repeated), _)
                    | (_, Some(JtKeyed::Repeated))
                    | (Some(_), Some(_)) => return Ok(None),
                    (Some(&JtKeyed::Unique(transform)), None) => (Some(transform), None),
                    (None, Some(&JtKeyed::Unique(material))) => (None, Some(material)),
                    (None, None) => (None, None),
                };
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
                    accumulate_display_jt_material(path, attribute);
                }
            }
            if let Some(instance_id) = instance_id {
                ctx.reserve_scoped_vec(&mut path.instance_storage, &mut path.instance_path,
                    1, "nx JT instance path nodes")?;
                path.instance_path.push(ctx.copy_scoped_text(instance_id,
                    &mut path.instance_storage, "nx JT instance path identity")?);
            }
            ctx.reserve_scoped_vec(
                &mut path.node_storage,
                &mut path.node_path,
                1,
                "nx JT node path nodes",
            )?;
            path.node_path.push(object_id);
        }
        Ok(Some(JtResolvedPaths { values: parent_states, storage: paths_storage }))
    }
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

/// Lookup tables from each mesh to the arrays and scene records it joins.
///
/// Identities are unique in decoded arrays, so a repeated key matches no record.
struct JtMeshIndex<'a, 'ctx> {
    coordinate_headers: HashMap<&'a str, Option<&'a DisplayJtVertexCoordinateArrayHeader>>,
    shape_elements: HashMap<&'a str, Option<&'a DisplayJtShapeLodElement>>,
    vertex_headers: HashMap<&'a str, Option<&'a DisplayJtCompressedVertexRecordsHeader>>,
    vertex_flags: HashMap<&'a str, Option<&'a DisplayJtVertexFlags>>,
    bindings: HashMap<(&'a str, u32), Option<&'a DisplayJtShapeLodBinding>>,
    shape_nodes: HashMap<(u32, &'a str), Option<&'a DisplayJtTriStripShapeNode>>,
    _storage: [ScopedReservation<'ctx>; 6],
}

impl<'a, 'ctx> JtMeshIndex<'a, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        inputs: &DisplayJtTessellationInputs<'a>,
        scene: &JtTessellationIndex<'a, '_>,
    ) -> Result<Self, CodecError> {
        let (coordinate_headers_candidate, coordinate_header_storage_candidate) = ctx.unique_index(
            inputs
                .coordinate_headers
                .iter()
                .map(|header| (header.id.as_str(), header)),
            "index JT mesh coordinate headers",
        )?;
        let coordinate_header_storage = coordinate_header_storage_candidate;
        let coordinate_headers = coordinate_headers_candidate;
        let (shape_elements_candidate, shape_element_storage_candidate) = ctx.unique_index(
            inputs
                .shape_elements
                .iter()
                .map(|element| (element.id.as_str(), element)),
            "index JT mesh shape elements",
        )?;
        let shape_element_storage = shape_element_storage_candidate;
        let shape_elements = shape_elements_candidate;
        let (vertex_headers_candidate, vertex_header_storage_candidate) = ctx.unique_index(
            inputs
                .vertex_headers
                .iter()
                .map(|header| (header.element.as_str(), header)),
            "index JT mesh vertex headers",
        )?;
        let vertex_header_storage = vertex_header_storage_candidate;
        let vertex_headers = vertex_headers_candidate;
        let (vertex_flags_candidate, vertex_flag_storage_candidate) = ctx.unique_index(
            inputs
                .vertex_flags
                .iter()
                .map(|flags| (flags.vertex_records_header.as_str(), flags)),
            "index JT mesh vertex flags",
        )?;
        let vertex_flag_storage = vertex_flag_storage_candidate;
        let vertex_flags = vertex_flags_candidate;
        let (bindings_candidate, binding_storage_candidate) = ctx.unique_index(
            inputs.bindings.iter().map(|binding| {
                (
                    (binding.shape_segment.as_str(), binding.payload_object_id),
                    binding,
                )
            }),
            "index JT mesh bindings",
        )?;
        let binding_storage = binding_storage_candidate;
        let bindings = bindings_candidate;
        let mut node_scene_storage = ctx.reserve_scoped(0, "index JT mesh shape nodes")?;
        let mut node_scenes = Vec::new();
        let mut remaining_shape_nodes = inputs.shape_nodes.iter();
        while !remaining_shape_nodes.as_slice().is_empty() {
            let Some(node) =
                ctx.next_charged(&mut remaining_shape_nodes, "index JT mesh shape nodes")?
            else {
                break;
            };
            let Some(base) = scene.base_node(ctx, &node.base_node)? else {
                continue;
            };
            let Some(segment) = scene.element_segment(ctx, &base.element)? else {
                continue;
            };
            ctx.push_scoped_vec(
                &mut node_scene_storage,
                &mut node_scenes,
                ((node.object_id, segment), node),
                "index JT mesh shape nodes",
            )?;
        }
        let (shape_nodes_candidate, shape_node_storage_candidate) =
            ctx.unique_index(node_scenes, "index JT mesh shape nodes")?;
        drop(node_scene_storage);
        let shape_node_storage = shape_node_storage_candidate;
        let shape_nodes = shape_nodes_candidate;
        Ok(Self {
            coordinate_headers,
            shape_elements,
            vertex_headers,
            vertex_flags,
            bindings,
            shape_nodes,
            _storage: [
                coordinate_header_storage,
                shape_element_storage,
                vertex_header_storage,
                vertex_flag_storage,
                binding_storage,
                shape_node_storage,
            ],
        })
    }
}

enum DisplayJtTessellationCandidateFailure<'ctx> {
    Codec(CodecError),
    Identity(JtTessellationIdentityRefusal<'ctx>),
    Lane(cadmpeg_ir::tessellation::TessellationLaneError),
    Validation(cadmpeg_ir::tessellation::TessellationError),
}

impl<'ctx> From<CodecError> for DisplayJtTessellationCandidateFailure<'ctx> {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

impl<'ctx> From<cadmpeg_core::decode::ResourceLimit>
    for DisplayJtTessellationCandidateFailure<'ctx>
{
    fn from(error: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Codec(CodecError::ResourceLimit(error))
    }
}

fn display_jt_tessellation_rows<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    inputs: &DisplayJtTessellationInputs<'_>,
) -> Result<Option<Vec<(Tessellation, u64)>>, CodecError> {
    let mut candidate_storage = ctx.reserve_scoped(0, "nx JT tessellation candidates")?;
    let candidate_result = candidate_storage.with_storage(|| {
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
        ..
    } = *inputs;
    if meshes.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let scene_index = JtTessellationIndex::new(ctx, inputs)?;
    let mesh_index = JtMeshIndex::new(ctx, inputs, &scene_index)?;
    let vertex_arrays =
        JtVertexArrayIndex::new(ctx, &[], coordinates, normals, colors, texture_coordinates)?;
    let mut scene_storage = ctx.reserve_scoped(0, "nx JT scene graphs")?;
    let mut scenes = BTreeMap::<&str, Option<JtSceneGraph<'_, '_>>>::new();
    let mut tessellations = Vec::new();
    let mut remaining_meshes = meshes.iter();
    while !remaining_meshes.as_slice().is_empty() {
        let Some(mesh) = ctx.next_charged(&mut remaining_meshes, "nx JT tessellation meshes")?
        else {
            break;
        };
        let coordinate_header = required!(ctx
            .get_hash_map(
                &mesh_index.coordinate_headers,
                mesh.coordinate_header.as_str(),
                "match JT mesh coordinate headers",
            )?
            .copied()
            .flatten());
        let coordinates = required!(ctx
            .get_hash_map(
                &vertex_arrays.coordinates,
                coordinate_header.id.as_str(),
                "match DisplayJT vertex coordinates",
            )?
            .copied()
            .flatten());
        let shape_element = required!(ctx
            .get_hash_map(
                &mesh_index.shape_elements,
                coordinate_header.element.as_str(),
                "match JT mesh shape elements",
            )?
            .copied()
            .flatten());
        let binding = required!(ctx
            .get_hash_map(
                &mesh_index.bindings,
                &(shape_element.segment.as_str(), shape_element.object_id),
                "match JT mesh bindings",
            )?
            .copied()
            .flatten());
        let shape_node = required!(ctx
            .get_hash_map(
                &mesh_index.shape_nodes,
                &(binding.shape_node_object_id, binding.scene_segment.as_str()),
                "match JT mesh shape nodes",
            )?
            .copied()
            .flatten());
        if ctx
            .get_btree_map(
                &scenes,
                binding.scene_segment.as_str(),
                "nx JT scene graphs",
            )?
            .is_none()
        {
            let graph = JtSceneGraph::new(ctx, &binding.scene_segment, inputs, &scene_index)?;
            scene_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut scenes,
                    binding.scene_segment.as_str(),
                    graph,
                    "nx JT scene graphs",
                )
            })?;
        }
        let graph = required!(ctx
            .get_btree_map(
                &scenes,
                binding.scene_segment.as_str(),
                "nx JT scene graphs"
            )?
            .and_then(Option::as_ref));
        let JtResolvedPaths { values: paths_candidate, storage: _paths_storage } =
            required!(graph.node_paths(ctx, shape_node.object_id)?);
        let mut remaining_paths = paths_candidate.into_iter();
        let render_count = ctx
            .admit_iter(&mesh.polygons, "nx JT rendered triangles")?
            .filter(|polygon| polygon.group >= 0)
            .count();
        let (rendered_candidate, render_storage_candidate) =
            ctx.scoped_vector_storage(render_count, "nx JT rendered triangles")?;
        let mut render_storage = render_storage_candidate;
        let mut rendered = rendered_candidate;
        let mut remaining_polygons = mesh.polygons.iter();
        while !remaining_polygons.as_slice().is_empty() {
            let Some(polygon) =
                ctx.next_charged(&mut remaining_polygons, "nx JT rendered triangles")?
            else {
                break;
            };
            if polygon.group < 0 {
                continue;
            }
            let corners: &[(u32, Option<u32>); 3] =
                required!(polygon.corners.as_slice().try_into().ok());
            let triangle = corners.map(|(vertex, _)| vertex);
            let attributes = corners.map(|(_, attribute)| attribute);
            ctx.reserve_scoped_vec(
                &mut render_storage,
                &mut rendered,
                1,
                "nx JT rendered triangles",
            )?;
            rendered.push((triangle, attributes));
        }
        if rendered.is_empty() {
            return Ok(None);
        }
        let vertex_header = required!(ctx
            .get_hash_map(
                &mesh_index.vertex_headers,
                shape_element.id.as_str(),
                "match JT mesh vertex headers",
            )?
            .copied()
            .flatten());
        let normal_array = if vertex_header.vertex_bindings & 0x8 != 0 {
            Some(required!(vertex_arrays.normals(ctx, vertex_header)?))
        } else {
            None
        };
        let color_array = if vertex_header.vertex_bindings & 0x30 != 0 {
            Some(required!(vertex_arrays.colors(ctx, vertex_header)?))
        } else {
            None
        };
        let mut texture_array_storage = ctx.reserve_scoped(0, "nx JT texture array references")?;
        let mut texture_arrays = Vec::new();
        for channel in 0..8_u8 {
            if !jt_texture_channel_bound(vertex_header.vertex_bindings, channel) {
                continue;
            }
            let array =
                required!(vertex_arrays.texture_coordinates(ctx, vertex_header, channel)?);
            ctx.reserve_scoped_vec(
                &mut texture_array_storage,
                &mut texture_arrays,
                1,
                "nx JT texture array references",
            )?;
            texture_arrays.push(array);
        }
        let vertex_flag_array = if vertex_header.vertex_bindings & 0x40 != 0 {
            Some(required!(ctx
                .get_hash_map(
                    &mesh_index.vertex_flags,
                    vertex_header.id.as_str(),
                    "match JT mesh vertex flags",
                )?
                .copied()
                .flatten()))
        } else {
            None
        };
        while remaining_paths.len() != 0 {
            let Some(path) = ctx.next_charged(&mut remaining_paths, "nx JT tessellation paths")?
            else {
                break;
            };
            let transform = path.matrix;
            let color = if color_array.is_none() || path.override_vertex_colors == Some(true) {
                display_jt_path_color(&path)
            } else {
                None
            };
            let DisplayJtPath { instance_path: candidate_instance_path, instance_storage, .. } = path;
            let instance_path = candidate_instance_path;
            let mut node_path_storage = ctx.reserve_scoped(0, "nx JT rendered node path")?;
            let node_path = node_path_storage.with_storage(|| {
                ctx.join_display_retained(path.node_path.iter(), "-", "nx JT rendered node path")
            })?;
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
                let mut vertices =
                    ctx.vector_storage(triangle_vertex_count, "nx JT tessellation vertices")?;
                let mut triangles =
                    ctx.vector_storage(rendered.len(), "nx JT tessellation triangles")?;
                // An unshaded mesh is stated by absence: no normal record, no
                // normal lane.
                let mut normal_vectors = match normal_array {
                    Some(_) => Some(
                        ctx.vector_storage(triangle_vertex_count, "nx JT tessellation normals")?,
                    ),
                    None => None,
                };
                let mut color_data = match color_array {
                    Some(_) => ctx.vector_storage(
                        required!(triangle_vertex_count.checked_mul(16)),
                        "nx JT tessellation colors",
                    )?,
                    None => Vec::new(),
                };
                let (texture_component_counts_candidate, texture_count_storage_candidate) = ctx
                    .scoped_vector_storage(
                        texture_arrays.len(),
                        "nx JT texture component counts",
                    )?;
                let mut texture_count_storage = texture_count_storage_candidate;
                let mut texture_component_counts = texture_component_counts_candidate;
                let mut remaining_texture_arrays = texture_arrays.iter();
                while !remaining_texture_arrays.as_slice().is_empty() {
                    let Some(array) = ctx.next_charged(
                        &mut remaining_texture_arrays,
                        "nx JT texture component counts",
                    )? else {
                        break;
                    };
                    let count = required!(array.values.first()).len();
                    if !(1..=4).contains(&count)
                        || !ctx.all_by(
                            &array.values,
                            |value| Ok(value.len() == count),
                            "nx JT texture component validation",
                        )?
                    {
                        return Ok(None);
                    }
                    ctx.reserve_scoped_vec(
                        &mut texture_count_storage,
                        &mut texture_component_counts,
                        1,
                        "nx JT texture component counts",
                    )?;
                    texture_component_counts.push(count);
                }
                let (texture_data_candidate, texture_data_storage_candidate) = ctx.scoped_vector_storage(
                    texture_component_counts.len(),
                    "nx JT tessellation texture buffers",
                )?;
                let mut texture_data_storage = texture_data_storage_candidate;
                let mut texture_data = texture_data_candidate;
                let mut remaining_component_counts = texture_component_counts.iter();
                while !remaining_component_counts.as_slice().is_empty() {
                    let Some(&component_count) = ctx.next_charged(
                        &mut remaining_component_counts,
                        "nx JT tessellation texture bytes",
                    )? else {
                        break;
                    };
                    let byte_count = required!(triangle_vertex_count
                        .checked_mul(component_count)
                        .and_then(|count| count.checked_mul(4)));
                    let data =
                        ctx.vector_storage(byte_count, "nx JT tessellation texture bytes")?;
                    ctx.reserve_scoped_vec(
                        &mut texture_data_storage,
                        &mut texture_data,
                        1,
                        "nx JT tessellation texture buffers",
                    )?;
                    texture_data.push(data);
                }
                let mut vertex_flag_data = match vertex_flag_array {
                    Some(_) => ctx.vector_storage(
                        required!(triangle_vertex_count.checked_mul(4)),
                        "nx JT tessellation flag bytes",
                    )?,
                    None => Vec::new(),
                };
                let mut remaining_rendered = rendered.iter().copied();
                while remaining_rendered.len() != 0 {
                    let Some((triangle, attributes)) = ctx.next_charged(
                        &mut remaining_rendered,
                        "nx JT tessellation triangles",
                    )? else {
                        break;
                    };
                    let base = required!(u32::try_from(vertices.len()).ok());
                    for (coordinate, attribute) in triangle.into_iter().zip(attributes) {
                        ctx.reserve_vec(&mut vertices, 1, "nx JT tessellation vertices")?;
                        vertices.push(required!(convert_point(coordinate)));
                        let attribute = required!(usize::try_from(required!(attribute)).ok());
                        if let (Some(normal_array), Some(normal_vectors)) =
                            (normal_array, normal_vectors.as_mut())
                        {
                            let normal = required!(normal_array.normals.get(attribute));
                            ctx.reserve_vec(normal_vectors, 1, "nx JT tessellation normals")?;
                            normal_vectors.push(FiniteVector3::from(required!(
                                transform_jt_normal(transform, normal.map(FiniteBinary32::get),)
                            )));
                        }
                        if let Some(color_array) = color_array {
                            for component in required!(color_array.colors.get(attribute)) {
                                ctx.extend_from_slice(
                                    &mut color_data,
                                    &component.get().to_le_bytes(),
                                    "nx JT tessellation colors",
                                )?;
                            }
                        }
                        let mut remaining_texture_arrays = texture_arrays.iter().enumerate();
                        while remaining_texture_arrays.len() != 0 {
                            let Some((index, array)) = ctx.next_charged(
                                &mut remaining_texture_arrays,
                                "nx JT tessellation texture bytes",
                            )? else {
                                break;
                            };
                            let data = required!(texture_data.get_mut(index));
                            let mut remaining_components =
                                required!(array.values.get(attribute)).iter();
                            while !remaining_components.as_slice().is_empty() {
                                let Some(component) = ctx.next_charged(
                                    &mut remaining_components,
                                    "nx JT tessellation texture bytes",
                                )? else {
                                    break;
                                };
                                ctx.extend_from_slice(
                                    data,
                                    &component.get().to_le_bytes(),
                                    "nx JT tessellation texture bytes",
                                )?;
                            }
                        }
                        if let Some(array) = vertex_flag_array {
                            ctx.extend_from_slice(
                                &mut vertex_flag_data,
                                &required!(array.values.get(attribute)).to_le_bytes(),
                                "nx JT tessellation flag bytes",
                            )?;
                        }
                    }
                    ctx.reserve_vec(&mut triangles, 1, "nx JT tessellation triangles")?;
                    triangles.push([
                        base,
                        required!(base.checked_add(1)),
                        required!(base.checked_add(2)),
                    ]);
                }
                let channel_count = required!(usize::from(color_array.is_some())
                    .checked_add(texture_arrays.len())
                    .and_then(|count| count.checked_add(usize::from(vertex_flag_array.is_some()))));
                let mut channels =
                    ctx.vector_storage(channel_count, "nx JT tessellation channels")?;
                if color_array.is_some() {
                    ctx.push_vec(
                        &mut channels,
                        required!(TessellationChannel::new(
                            cadmpeg_ir::tessellation::ChannelAddressing::Vertex {},
                            16,
                            DISPLAY_JT_COLOR_CHANNEL,
                            required!(
                                u32::try_from((vertex_header.vertex_bindings >> 4) & 0x3).ok()
                            ),
                            color_data,
                        )
                        .ok()),
                        "nx JT tessellation channels",
                    )?;
                }
                let mut component_counts = texture_component_counts.into_iter();
                let mut data_buffers = texture_data.into_iter();
                let mut remaining_channels = texture_arrays.iter().enumerate();
                while remaining_channels.len() != 0 {
                    let Some((ordinal, array)) = ctx.next_charged(
                        &mut remaining_channels,
                        "nx JT tessellation channels",
                    )? else {
                        break;
                    };
                    let (Some(component_count), Some(data)) =
                        (component_counts.next(), data_buffers.next())
                    else {
                        break;
                    };
                    ctx.push_vec(
                        &mut channels,
                        required!(
                            TessellationChannel::new(
                                cadmpeg_ir::tessellation::ChannelAddressing::Vertex {},
                                required!(
                                    u32::try_from(required!(component_count.checked_mul(4))).ok()
                                ),
                                required!(DISPLAY_JT_TEXTURE_CHANNEL_BASE.checked_add(required!(
                                    u32::try_from(ordinal).ok()
                                ))),
                                u32::from(array.channel)
                                    | required!(u32::try_from(
                                        (vertex_header.vertex_bindings >> (8 + 4 * array.channel))
                                            & 0xf
                                    )
                                    .ok())
                                        << 8,
                                data,
                            )
                            .ok()
                        ),
                        "nx JT tessellation channels",
                    )?;
                }
                if vertex_flag_array.is_some() {
                    ctx.push_vec(
                        &mut channels,
                        required!(TessellationChannel::new(
                            cadmpeg_ir::tessellation::ChannelAddressing::Vertex {},
                            4,
                            DISPLAY_JT_VERTEX_FLAG_CHANNEL,
                            0,
                            vertex_flag_data,
                        )
                        .ok()),
                        "nx JT tessellation channels",
                    )?;
                }
                (vertices, triangles, normal_vectors, channels)
            } else {
                let mut vertices =
                    ctx.vector_storage(coordinates.points_m.len(), "nx JT tessellation vertices")?;
                let mut point_indices = 0..coordinates.points_m.len();
                while point_indices.len() != 0 {
                    let Some(index) =
                        ctx.next_charged(&mut point_indices, "nx JT tessellation vertices")?
                    else {
                        break;
                    };
                    ctx.reserve_vec(&mut vertices, 1, "nx JT tessellation vertices")?;
                    vertices.push(required!(convert_point(required!(
                        u32::try_from(index).ok()
                    ))));
                }
                let mut triangles =
                    ctx.vector_storage(rendered.len(), "nx JT tessellation triangles")?;
                let mut remaining_rendered = rendered.iter();
                while !remaining_rendered.as_slice().is_empty() {
                    let Some((triangle, _)) = ctx
                        .next_charged(&mut remaining_rendered, "nx JT tessellation triangles")?
                    else {
                        break;
                    };
                    ctx.reserve_vec(&mut triangles, 1, "nx JT tessellation triangles")?;
                    triangles.push(*triangle);
                }
                (vertices, triangles, None, Vec::new())
            };
            ctx.charge_entities(1, "nx JT tessellations")?;
            let tessellation_id = match retain_jt_tessellation_id(
                ctx,
                shape_element.source_offset,
                shape_element.object_id,
                (path.node_path.len() != 1).then_some(node_path.as_str()),
            )? {
                Ok(id) => id,
                Err(refusal) => {
                    return Err(DisplayJtTessellationCandidateFailure::Identity(refusal));
                }
            };
            let mesh = match cadmpeg_ir::tessellation::TessellationMesh::from_checked_list_lanes(
                vertices,
                triangles,
                normal_vectors,
            ) {
                Ok(mesh) => mesh,
                Err(error) => return Err(DisplayJtTessellationCandidateFailure::Lane(error)),
            };
            let tessellation = Tessellation::from_parts(tessellation_id, mesh, channels)
                .map_err(DisplayJtTessellationCandidateFailure::Validation)?;
            instance_storage.commit()?;
            ctx.push_vec(
                &mut tessellations,
                (
                    tessellation.with_source_object(Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Nx,
                        object_id: required!(cadmpeg_core::text::NonBlankString::for_decode(
                            ctx,
                            (ctx.join_retained(
                                &[&shape_node.id],
                                "",
                                "nx JT tessellation source identity"
                            ))?,
                            "validate nonblank text"
                        )?),
                        name: None,
                        color,
                        visible: None,
                        layer: None,
                        instance_path,
                    })),
                    shape_node.source_offset,
                ),
                "nx JT tessellations",
            )?;
        }
    }
    Ok(Some(tessellations))
    });
    match candidate_result {
        Ok(Some(rows)) => {
            if !rows.is_empty() {
                candidate_storage.commit()?;
            }
            Ok(Some(rows))
        }
        Ok(None) => Ok(None),
        Err(DisplayJtTessellationCandidateFailure::Identity(refusal)) => {
            drop(candidate_storage);
            let message = ctx.format_retained(
                format_args!("display-jt tessellation: {}", refusal.error),
                "retain DisplayJT tessellation rejection",
            )?;
            drop(refusal);
            Err(CodecError::Malformed(message))
        }
        Err(DisplayJtTessellationCandidateFailure::Lane(error)) => {
            drop(candidate_storage);
            let message = ctx.format_retained(
                format_args!("display-jt tessellation: {error}"),
                "retain DisplayJT tessellation rejection",
            )?;
            Err(CodecError::Malformed(message))
        }
        Err(DisplayJtTessellationCandidateFailure::Validation(error)) => {
            drop(candidate_storage);
            let message = ctx.format_retained(
                format_args!("display-jt tessellation: {error}"),
                "retain DisplayJT tessellation rejection",
            )?;
            Err(CodecError::Malformed(message))
        }
        Err(DisplayJtTessellationCandidateFailure::Codec(error)) => Err(error),
    }
}

#[cfg(test)]
mod tests;

// Each optional key below names itself in whatever it refuses.
cadmpeg_core::named_optional_field!(deserialize_reflectivity, f32, "reflectivity");
