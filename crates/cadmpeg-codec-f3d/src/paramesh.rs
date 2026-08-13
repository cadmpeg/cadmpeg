// SPDX-License-Identifier: Apache-2.0
//! Decode `.paramesh` mesh-geometry containers
//! ([spec §1.1.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#112-mesh-geometry-containers)).
//!
//! One container holds one mesh body's geometry: a protobuf registry followed
//! by chunks. Registry fields 21 and 22 name the streams holding f32 vertex
//! triples and delta-coded triangle corners. Vertex coordinates are container
//! coordinates; the mesh-body Design record stores the affine map that places
//! them in model space.
//!
//! The registry declares attribute channels with value and index streams. Vertex
//! channels store one value per vertex. Indexed channels store one default value
//! per vertex followed by corner overrides selected by a delta-coded position
//! stream.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

/// Container magic.
const MAGIC: [u8; 12] = [
    0x89, 0x55, 0x44, 0x50, 0x4D, 0x45, 0x53, 0x48, 0x0D, 0x0A, 0x1A, 0x0A,
];
/// Container version accepted by this decoder.
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
/// Raw-stream selector following the common properties byte.
const RAW_STREAM_MODE: u8 = 0xfe;
/// Largest stream this decoder inflates.
const MAX_STREAM_BYTES: u32 = 64 * 1024 * 1024;
/// Registry field that declares one vertex-domain attribute channel.
const REGISTRY_VERTEX_CHANNEL: u64 = 4;
/// Registry field that declares one triangle-domain attribute channel.
const REGISTRY_TRIANGLE_CHANNEL: u64 = 5;
/// Registry field that names the stream of classified feature-edge endpoints.
const REGISTRY_FEATURE_EDGES: u64 = 7;
/// Registry field holding one key/value property entry.
const REGISTRY_PROPERTY: u64 = 8;
/// Registry field storing the number of face-group records.
const REGISTRY_FACE_GROUP_COUNT: u64 = 9;
/// Registry field storing the container-local mesh UUID.
const REGISTRY_MESH_UUID: u64 = 12;
/// Registry field naming the vertex-position stream.
const REGISTRY_VERTICES: u64 = 21;
/// Registry field naming the triangle-corner stream.
const REGISTRY_TRIANGLES: u64 = 22;
/// Nested feature-edge field holding the stream name.
const FEATURE_EDGE_STREAM: u64 = 4;
/// Property-entry field holding the property key.
const PROPERTY_KEY: u64 = 1;
/// Property-entry field holding a text value.
const PROPERTY_TEXT: u64 = 3;
/// Property-entry field holding a stream name.
const PROPERTY_STREAM: u64 = 4;
/// Channel field holding the role the attribute plays.
const CHANNEL_ROLE: u64 = 2;
/// Channel field holding the attribute resource GUID.
const CHANNEL_RESOURCE: u64 = 3;
/// Channel field holding the stream names and the element code.
const CHANNEL_STREAMS: u64 = 5;
/// Channel field mapping one face-group key to its GUID.
const CHANNEL_GROUP: u64 = 6;
/// Face-group entry field holding the numeric group key.
const GROUP_KEY: u64 = 1;
/// Face-group entry field holding the group GUID.
const GROUP_GUID: u64 = 2;
/// Stream-entry field holding the element code.
const STREAM_ELEMENT_CODE: u64 = 1;
/// Stream-entry field holding the value-stream name.
const STREAM_VALUES: u64 = 2;
/// Stream-entry field holding the index-stream name.
const STREAM_INDEX: u64 = 3;
/// Element code of a two-component `f32` element.
const ELEMENT_PAIR: u64 = 2;
/// Element code of a four-component `f32` element.
const ELEMENT_QUAD: u64 = 4;
/// Element code of a three-component direction packed into two `f32` values.
const ELEMENT_PACKED_DIRECTION: u64 = 5;
/// Element code of one delta-coded value per triangle.
const ELEMENT_TRIANGLE_DELTA: u64 = 7;
/// Bytes of one [`ELEMENT_PACKED_DIRECTION`] element.
const PACKED_DIRECTION_BYTES: u32 = 8;

/// One mesh body's decoded geometry.
pub(crate) struct MeshContainer {
    /// The ASCII GUID the protobuf message carries as `fusion_uuid`, which the
    /// container's Design-segment GUID record repeats.
    pub(crate) fusion_uuid: String,
    /// Container-local version-4 UUID stored in registry field 12.
    pub(crate) mesh_uuid: String,
    /// One coordinate triple per vertex, in container coordinates.
    pub(crate) vertices: Vec<[f64; 3]>,
    /// Triangle corner indices into `vertices`.
    pub(crate) triangles: Vec<[u32; 3]>,
    /// Source-classified feature edges as ascending vertex-index pairs.
    pub(crate) feature_edges: Vec<[u32; 2]>,
    /// One decoded unit normal per flattened triangle corner.
    pub(crate) corner_normals: Vec<[f64; 3]>,
    /// Source face groups as an ordered partition of triangle ordinals.
    pub(crate) triangle_groups: Vec<MeshTriangleGroup>,
    /// One texture-table selector per triangle, when the `tid` channel exists.
    pub(crate) texture_ids: Option<Vec<u32>>,
    /// The attribute channels the registry declares, in registry order.
    pub(crate) attributes: Vec<MeshAttribute>,
}

/// The domain over which an attribute channel's values are addressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MeshAttributeDomain {
    /// One value per vertex. The registry declares no index stream, and the
    /// value count equals the vertex count.
    Vertex,
    /// One value per triangle corner, deduplicated per vertex. The index
    /// stream selects a value for each corner.
    Corner,
    /// One value per triangle.
    Triangle,
}

/// One attribute channel a container's registry declares.
pub(crate) struct MeshAttribute {
    /// The role the registry records for the attribute.
    pub(crate) role: u32,
    /// Registry field-3 resource GUID, when the channel has one.
    pub(crate) resource_guid: Option<String>,
    /// Authored attribute name resolved through `attname.amt.autodesk`.
    pub(crate) authored_name: Option<String>,
    /// Face-group key/GUID records carried by repeated field 6 entries.
    pub(crate) groups: Vec<(u32, String)>,
    /// The registry's element code: the count of `f32` components of one
    /// element for [`ELEMENT_PAIR`] and [`ELEMENT_QUAD`], and a packed or
    /// delta-coded form otherwise.
    pub(crate) element_code: u32,
    /// Which entities the values address.
    pub(crate) domain: MeshAttributeDomain,
    /// Bytes of one element, when the element code settles the element width.
    pub(crate) item_size: Option<u32>,
    /// The value stream, verbatim.
    pub(crate) values: Vec<u8>,
    /// Explicit corner positions selected by the optional index stream.
    pub(crate) indices: Option<Vec<u32>>,
    /// Decoded terminal-delta values for a triangle-domain code-7 channel.
    pub(crate) triangle_values: Option<Vec<u32>>,
}

/// One source face group and its triangle membership.
pub(crate) struct MeshTriangleGroup {
    /// Container-local group GUID.
    pub(crate) source_id: String,
    /// Strictly increasing triangle ordinals in the group.
    pub(crate) triangles: Vec<u32>,
}

impl MeshAttribute {
    /// The element count, when the element width is settled.
    pub(crate) fn count(&self) -> Option<u32> {
        let item_size = usize::try_from(self.item_size?).ok()?;
        self.values
            .len()
            .checked_div(item_size)
            .filter(|_| self.values.len().is_multiple_of(item_size))
            .and_then(|count| u32::try_from(count).ok())
    }

    /// Expand a vertex- or corner-domain value table to one table selector per
    /// flattened triangle corner.
    pub(crate) fn corner_selectors(
        &self,
        vertices: usize,
        triangles: &[[u32; 3]],
    ) -> Option<Vec<u32>> {
        let count = self.count()?;
        let vertex_count = u32::try_from(vertices).ok()?;
        let positions = match self.domain {
            MeshAttributeDomain::Vertex => {
                (count == vertex_count && self.indices.is_none()).then_some(&[][..])?
            }
            MeshAttributeDomain::Corner => {
                let positions = self.indices.as_deref()?;
                let overrides = count.checked_sub(vertex_count)?;
                (usize::try_from(overrides).ok()? == positions.len()).then_some(positions)?
            }
            MeshAttributeDomain::Triangle => return None,
        };

        let mut selectors = Vec::with_capacity(triangles.len().checked_mul(3)?);
        for triangle in triangles {
            for vertex in triangle {
                if usize::try_from(*vertex)
                    .ok()
                    .is_none_or(|index| index >= vertices)
                    || *vertex >= count
                {
                    return None;
                }
                selectors.push(*vertex);
            }
        }
        for (ordinal, position) in positions.iter().enumerate() {
            let selector = vertex_count.checked_add(u32::try_from(ordinal).ok()?)?;
            *selectors.get_mut(usize::try_from(*position).ok()?)? = selector;
        }
        Some(selectors)
    }
}

/// Read one bounded protobuf varint.
fn take_varint(message: &[u8], at: &mut usize) -> Result<u64, CodecError> {
    let mut value = 0u64;
    for ordinal in 0..10u32 {
        let byte = *message
            .get(*at)
            .ok_or_else(|| malformed("paramesh protobuf varint is truncated"))?;
        *at += 1;
        if ordinal == 9 && byte > 1 {
            return Err(malformed("paramesh protobuf varint exceeds u64"));
        }
        value |= u64::from(byte & 0x7f) << (ordinal * 7);
        if byte < 0x80 {
            return Ok(value);
        }
    }
    Err(malformed("paramesh protobuf varint exceeds ten bytes"))
}

/// One protobuf field value. Fixed-width values are retained only as a wire
/// shape because the implemented registry fields do not interpret them.
enum ProtobufValue<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed64,
    Fixed32,
}

/// Read every protobuf field in stored order.
fn protobuf_fields(message: &[u8]) -> Result<Vec<(u64, ProtobufValue<'_>)>, CodecError> {
    let mut fields = Vec::new();
    let mut at = 0usize;
    while at < message.len() {
        let key = take_varint(message, &mut at)?;
        if key >> 3 == 0 {
            return Err(malformed("paramesh protobuf field number is zero"));
        }
        match key & 0x07 {
            0 => {
                let value = take_varint(message, &mut at)?;
                fields.push((key >> 3, ProtobufValue::Varint(value)));
            }
            1 => {
                at = at
                    .checked_add(8)
                    .filter(|end| *end <= message.len())
                    .ok_or_else(|| malformed("paramesh protobuf fixed64 field is truncated"))?;
                fields.push((key >> 3, ProtobufValue::Fixed64));
            }
            2 => {
                let count = usize::try_from(take_varint(message, &mut at)?)
                    .map_err(|_| malformed("paramesh protobuf byte count is out of range"))?;
                let bytes = at
                    .checked_add(count)
                    .and_then(|end| message.get(at..end))
                    .ok_or_else(|| malformed("paramesh protobuf byte field is truncated"))?;
                at += count;
                fields.push((key >> 3, ProtobufValue::Bytes(bytes)));
            }
            5 => {
                at = at
                    .checked_add(4)
                    .filter(|end| *end <= message.len())
                    .ok_or_else(|| malformed("paramesh protobuf fixed32 field is truncated"))?;
                fields.push((key >> 3, ProtobufValue::Fixed32));
            }
            _ => {
                return Err(malformed(
                    "paramesh protobuf message uses an unsupported group wire type",
                ));
            }
        }
    }
    Ok(fields)
}

/// One channel's declared element code and stream names.
struct ChannelStreams<'a> {
    element_code: u64,
    values: &'a str,
    index: Option<&'a str>,
}

/// Complete registry declaration of one attribute channel.
struct RegisteredChannel<'a> {
    streams: ChannelStreams<'a>,
    role: u32,
    domain: MeshAttributeDomain,
    resource_guid: Option<String>,
    groups: Vec<(u32, String)>,
}

/// One registry property value.
enum RegistryProperty {
    Text(String),
    Stream(String),
}

#[derive(Clone, Copy)]
enum AttributeNameKind {
    Color,
    Group,
    TextureCoordinate,
}

struct RegisteredAttributeName {
    kind: AttributeNameKind,
    authored_name: String,
}

/// Singleton metadata and properties carried by the top-level registry.
struct MeshRegistry {
    fusion_uuid: String,
    mesh_uuid: String,
    face_group_count: u32,
    vertex_stream: String,
    triangle_stream: String,
    attribute_name_stream: Option<String>,
}

fn guid(bytes: &[u8], context: &str) -> Result<String, CodecError> {
    let value = std::str::from_utf8(bytes)
        .map_err(|_| malformed(format!("paramesh {context} is not ASCII")))?;
    if !crate::bytes::is_guid_hyphenated(value) {
        return Err(malformed(format!("paramesh {context} is not a GUID")));
    }
    Ok(value.to_owned())
}

fn utf8(bytes: &[u8], context: &str) -> Result<String, CodecError> {
    let value = std::str::from_utf8(bytes)
        .map_err(|_| malformed(format!("paramesh {context} is not UTF-8")))?;
    if value.is_empty() {
        return Err(malformed(format!("paramesh {context} is empty")));
    }
    Ok(value.to_owned())
}

/// Read the stream entry of one channel.
fn channel_streams(entry: &[u8]) -> Result<ChannelStreams<'_>, CodecError> {
    let mut element_code = None;
    let mut values = None;
    let mut index = None;
    for (field, value) in protobuf_fields(entry)? {
        match (field, value) {
            (STREAM_ELEMENT_CODE, ProtobufValue::Varint(code)) => {
                if element_code.replace(code).is_some() {
                    return Err(malformed("paramesh channel repeats its element code"));
                }
            }
            (STREAM_VALUES, ProtobufValue::Bytes(name)) => {
                let name = std::str::from_utf8(name)
                    .map_err(|_| malformed("paramesh channel value-stream name is not UTF-8"))?;
                if values.replace(name).is_some() {
                    return Err(malformed("paramesh channel repeats its value-stream name"));
                }
            }
            (STREAM_INDEX, ProtobufValue::Bytes(name)) => {
                let name = std::str::from_utf8(name)
                    .map_err(|_| malformed("paramesh channel index-stream name is not UTF-8"))?;
                if index.replace(name).is_some() {
                    return Err(malformed("paramesh channel repeats its index-stream name"));
                }
            }
            (STREAM_ELEMENT_CODE | STREAM_VALUES | STREAM_INDEX, _) => {
                return Err(malformed(
                    "paramesh channel stream field has the wrong wire type",
                ));
            }
            _ => return Err(malformed("paramesh channel stream has an undefined field")),
        }
    }
    Ok(ChannelStreams {
        element_code: element_code
            .ok_or_else(|| malformed("paramesh channel has no element code"))?,
        values: values.ok_or_else(|| malformed("paramesh channel has no value-stream name"))?,
        index,
    })
}

/// Read one face-group key/GUID map entry.
fn channel_group(entry: &[u8]) -> Result<(u32, String), CodecError> {
    let mut key = None;
    let mut group_guid = None;
    for (field, value) in protobuf_fields(entry)? {
        match (field, value) {
            (GROUP_KEY, ProtobufValue::Varint(value)) => {
                let value = u32::try_from(value)
                    .map_err(|_| malformed("paramesh face-group key is out of range"))?;
                if key.replace(value).is_some() {
                    return Err(malformed("paramesh face-group entry repeats its key"));
                }
            }
            (GROUP_GUID, ProtobufValue::Bytes(value)) => {
                let value = guid(value, "face-group identity")?;
                if group_guid.replace(value).is_some() {
                    return Err(malformed("paramesh face-group entry repeats its GUID"));
                }
            }
            (GROUP_KEY | GROUP_GUID, _) => {
                return Err(malformed(
                    "paramesh face-group field has the wrong wire type",
                ));
            }
            _ => {
                return Err(malformed(
                    "paramesh face-group entry has an undefined field",
                ))
            }
        }
    }
    Ok((
        key.ok_or_else(|| malformed("paramesh face-group entry has no key"))?,
        group_guid.ok_or_else(|| malformed("paramesh face-group entry has no GUID"))?,
    ))
}

/// Read one channel entry of the registry.
fn registry_channel(
    entry: &[u8],
    domain: MeshAttributeDomain,
) -> Result<RegisteredChannel<'_>, CodecError> {
    let mut role = 0u32;
    let mut has_role = false;
    let mut resource_guid = None;
    let mut streams = None;
    let mut groups: Vec<(u32, String)> = Vec::new();
    for (field, value) in protobuf_fields(entry)? {
        match (field, value) {
            (CHANNEL_ROLE, ProtobufValue::Varint(value)) => {
                if has_role {
                    return Err(malformed("paramesh channel repeats its role"));
                }
                has_role = true;
                role = u32::try_from(value)
                    .map_err(|_| malformed("paramesh channel role is out of range"))?;
            }
            (CHANNEL_RESOURCE, ProtobufValue::Bytes(value)) => {
                let value = guid(value, "channel resource identity")?;
                if resource_guid.replace(value).is_some() {
                    return Err(malformed("paramesh channel repeats its resource GUID"));
                }
            }
            (CHANNEL_STREAMS, ProtobufValue::Bytes(nested)) => {
                if streams.replace(channel_streams(nested)?).is_some() {
                    return Err(malformed("paramesh channel repeats its stream entry"));
                }
            }
            (CHANNEL_GROUP, ProtobufValue::Bytes(nested)) => {
                let group = channel_group(nested)?;
                if groups.iter().any(|(key, group_guid)| {
                    *key == group.0 || group_guid.eq_ignore_ascii_case(&group.1)
                }) {
                    return Err(malformed(
                        "paramesh channel repeats a face-group key or GUID",
                    ));
                }
                groups.push(group);
            }
            (CHANNEL_ROLE | CHANNEL_RESOURCE | CHANNEL_STREAMS | CHANNEL_GROUP, _) => {
                return Err(malformed("paramesh channel field has the wrong wire type"));
            }
            _ => return Err(malformed("paramesh channel has an undefined field")),
        }
    }
    let streams = streams.ok_or_else(|| malformed("paramesh channel has no stream entry"))?;
    // A vertex-domain channel that carries an index stream addresses triangle
    // corners; without one it stores exactly one value per vertex.
    let domain = match (domain, streams.index) {
        (MeshAttributeDomain::Vertex, Some(_)) => MeshAttributeDomain::Corner,
        (domain, _) => domain,
    };
    Ok(RegisteredChannel {
        streams,
        role,
        domain,
        resource_guid,
        groups,
    })
}

/// The bytes of one element under an element code, when the code settles it.
fn element_bytes(element_code: u64) -> Option<u32> {
    match element_code {
        ELEMENT_PAIR | ELEMENT_QUAD => u32::try_from(element_code * 4).ok(),
        ELEMENT_PACKED_DIRECTION => Some(PACKED_DIRECTION_BYTES),
        ELEMENT_TRIANGLE_DELTA => Some(4),
        _ => None,
    }
}

fn malformed(message: impl Into<String>) -> CodecError {
    CodecError::Malformed(message.into())
}

/// Whether a registry field-12 value is a lowercase RFC 4122 version-4 UUID.
pub(crate) fn valid_mesh_uuid(value: &str) -> bool {
    if !crate::bytes::is_guid_hyphenated(value) {
        return false;
    }
    let bytes = value.as_bytes();
    !bytes.iter().any(u8::is_ascii_uppercase)
        && bytes[14] == b'4'
        && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
}

fn registry_property(entry: &[u8]) -> Result<(String, RegistryProperty), CodecError> {
    let mut key = None;
    let mut value = None;
    for (field, field_value) in protobuf_fields(entry)? {
        match (field, field_value) {
            (PROPERTY_KEY, ProtobufValue::Bytes(bytes)) => {
                let bytes = utf8(bytes, "property key")?;
                if key.replace(bytes).is_some() {
                    return Err(malformed("paramesh property repeats its key"));
                }
            }
            (PROPERTY_TEXT, ProtobufValue::Bytes(bytes)) => {
                let bytes = utf8(bytes, "property text")?;
                if value.replace(RegistryProperty::Text(bytes)).is_some() {
                    return Err(malformed("paramesh property repeats its value"));
                }
            }
            (PROPERTY_STREAM, ProtobufValue::Bytes(bytes)) => {
                let bytes = utf8(bytes, "property stream name")?;
                if value.replace(RegistryProperty::Stream(bytes)).is_some() {
                    return Err(malformed("paramesh property repeats its value"));
                }
            }
            (PROPERTY_KEY | PROPERTY_TEXT | PROPERTY_STREAM, _) => {
                return Err(malformed("paramesh property field has the wrong wire type"));
            }
            _ => return Err(malformed("paramesh property has an undefined field")),
        }
    }
    Ok((
        key.ok_or_else(|| malformed("paramesh property has no key"))?,
        value.ok_or_else(|| malformed("paramesh property has no value"))?,
    ))
}

/// Decode every singleton and property in the version-2 top-level registry.
fn mesh_registry(message: &[u8]) -> Result<MeshRegistry, CodecError> {
    let mut properties = std::collections::BTreeMap::new();
    let mut face_group_count = None;
    let mut mesh_uuid = None;
    let mut vertex_stream = None;
    let mut triangle_stream = None;
    for (field, value) in protobuf_fields(message)? {
        match (field, value) {
            (REGISTRY_VERTEX_CHANNEL | REGISTRY_TRIANGLE_CHANNEL | REGISTRY_FEATURE_EDGES, _) => {}
            (REGISTRY_PROPERTY, ProtobufValue::Bytes(entry)) => {
                let (key, value) = registry_property(entry)?;
                if properties.insert(key, value).is_some() {
                    return Err(malformed("paramesh registry repeats a property key"));
                }
            }
            (REGISTRY_FACE_GROUP_COUNT, ProtobufValue::Varint(value)) => {
                let value = u32::try_from(value)
                    .map_err(|_| malformed("paramesh face-group count is out of range"))?;
                if face_group_count.replace(value).is_some() {
                    return Err(malformed("paramesh registry repeats its face-group count"));
                }
            }
            (REGISTRY_MESH_UUID, ProtobufValue::Bytes(value)) => {
                let value = guid(value, "mesh UUID")?;
                if !valid_mesh_uuid(&value) {
                    return Err(malformed(
                        "paramesh mesh UUID is not a lowercase version-4 UUID",
                    ));
                }
                if mesh_uuid.replace(value).is_some() {
                    return Err(malformed("paramesh registry repeats its mesh UUID"));
                }
            }
            (REGISTRY_VERTICES, ProtobufValue::Bytes(value)) => {
                let value = utf8(value, "vertex-stream name")?;
                if vertex_stream.replace(value).is_some() {
                    return Err(malformed(
                        "paramesh registry repeats its vertex-stream name",
                    ));
                }
            }
            (REGISTRY_TRIANGLES, ProtobufValue::Bytes(value)) => {
                let value = utf8(value, "triangle-stream name")?;
                if triangle_stream.replace(value).is_some() {
                    return Err(malformed(
                        "paramesh registry repeats its triangle-stream name",
                    ));
                }
            }
            (
                REGISTRY_PROPERTY
                | REGISTRY_FACE_GROUP_COUNT
                | REGISTRY_MESH_UUID
                | REGISTRY_VERTICES
                | REGISTRY_TRIANGLES,
                _,
            ) => return Err(malformed("paramesh registry field has the wrong wire type")),
            _ => return Err(malformed("paramesh registry has an undefined field")),
        }
    }

    let fusion_uuid = match properties.remove("fusion_uuid") {
        Some(RegistryProperty::Text(value)) => guid(value.as_bytes(), "fusion_uuid")?,
        Some(RegistryProperty::Stream(_)) => {
            return Err(malformed("paramesh fusion_uuid property is not text"));
        }
        None => return Err(malformed("paramesh registry has no fusion_uuid property")),
    };
    let attribute_name_stream = match properties.remove("attname.amt.autodesk") {
        Some(RegistryProperty::Stream(value)) => Some(value),
        Some(RegistryProperty::Text(_)) => {
            return Err(malformed(
                "paramesh attribute-name property does not name a stream",
            ));
        }
        None => None,
    };
    Ok(MeshRegistry {
        fusion_uuid,
        mesh_uuid: mesh_uuid.ok_or_else(|| malformed("paramesh registry has no mesh UUID"))?,
        face_group_count: face_group_count
            .ok_or_else(|| malformed("paramesh registry has no face-group count"))?,
        vertex_stream: vertex_stream
            .ok_or_else(|| malformed("paramesh registry has no vertex-stream name"))?,
        triangle_stream: triangle_stream
            .ok_or_else(|| malformed("paramesh registry has no triangle-stream name"))?,
        attribute_name_stream,
    })
}

/// Read one `MessagePack` value from a stream-name table. The table uses string
/// keys and integer values.
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
            let count = usize::from(
                View::u16_be_at(bytes, at)
                    .ok_or_else(|| malformed("paramesh name table is truncated"))?,
            );
            at += 2;
            count
        }
        _ => return Err(malformed("paramesh name table is not a MessagePack map")),
    };
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let name = take_string(bytes, &mut at)?;
        if name.is_empty() {
            return Err(malformed("paramesh name table has an empty stream name"));
        }
        let id = take_integer(bytes, &mut at)?;
        if entries
            .iter()
            .any(|(existing_name, existing_id)| existing_name == &name || *existing_id == id)
        {
            return Err(malformed("paramesh name table repeats a stream name or id"));
        }
        entries.push((name, id));
    }
    if at != bytes.len() {
        return Err(malformed("paramesh name table has trailing bytes"));
    }
    Ok(entries)
}

/// Descriptor and decompressed bytes of one named stream.
struct MeshStream {
    descriptor: Vec<(String, StreamDescriptorValue)>,
    bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamDescriptorValue {
    Integer(u64),
    Boolean(bool),
}

/// Read the scalar-valued `MessagePack` descriptor map of one kind-4 chunk.
fn stream_descriptor(bytes: &[u8]) -> Result<Vec<(String, StreamDescriptorValue)>, CodecError> {
    fn value(bytes: &[u8], at: &mut usize) -> Result<StreamDescriptorValue, CodecError> {
        let tag = *bytes
            .get(*at)
            .ok_or_else(|| malformed("paramesh stream descriptor is truncated"))?;
        *at += 1;
        let width = match tag {
            0x00..=0x7f => return Ok(StreamDescriptorValue::Integer(u64::from(tag))),
            0xc2 => return Ok(StreamDescriptorValue::Boolean(false)),
            0xc3 => return Ok(StreamDescriptorValue::Boolean(true)),
            0xcc => 1,
            0xcd => 2,
            0xce => 4,
            0xcf => 8,
            _ => {
                return Err(malformed(
                    "paramesh stream descriptor value is not a supported scalar",
                ))
            }
        };
        let raw = bytes
            .get(*at..*at + width)
            .ok_or_else(|| malformed("paramesh stream descriptor is truncated"))?;
        *at += width;
        Ok(StreamDescriptorValue::Integer(
            raw.iter()
                .fold(0, |value, byte| (value << 8) | u64::from(*byte)),
        ))
    }

    let mut at = 0usize;
    let tag = *bytes
        .get(at)
        .ok_or_else(|| malformed("paramesh stream descriptor is empty"))?;
    at += 1;
    let count = match tag {
        0x80..=0x8f => usize::from(tag & 0x0f),
        _ => {
            return Err(malformed(
                "paramesh stream descriptor is not a MessagePack map",
            ))
        }
    };
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let tag = *bytes
            .get(at)
            .ok_or_else(|| malformed("paramesh stream descriptor is truncated"))?;
        at += 1;
        let key_count = match tag {
            0xa0..=0xbf => usize::from(tag & 0x1f),
            _ => return Err(malformed("paramesh stream descriptor key is not a string")),
        };
        let raw = bytes
            .get(at..at + key_count)
            .ok_or_else(|| malformed("paramesh stream descriptor is truncated"))?;
        at += key_count;
        let key = std::str::from_utf8(raw)
            .map_err(|_| malformed("paramesh stream descriptor key is not UTF-8"))?
            .to_owned();
        if entries.iter().any(|(existing, _)| existing == &key) {
            return Err(malformed("paramesh stream descriptor repeats a key"));
        }
        entries.push((key, value(bytes, &mut at)?));
    }
    if at != bytes.len() {
        return Err(malformed("paramesh stream descriptor has trailing bytes"));
    }
    Ok(entries)
}

/// Inflate one kind-4 chunk body: the descriptor map, the uncompressed byte
/// count, the two LZMA1 property bytes, and the raw LZMA1 stream.
fn inflate_stream(body: &[u8]) -> Result<MeshStream, CodecError> {
    let descriptor_count = usize::from(
        View::u16_le_at(body, 0).ok_or_else(|| malformed("paramesh stream chunk is truncated"))?,
    );
    let at = descriptor_count
        .checked_add(2)
        .ok_or_else(|| malformed("paramesh stream chunk is out of range"))?;
    let descriptor = stream_descriptor(
        body.get(2..at)
            .ok_or_else(|| malformed("paramesh stream descriptor is truncated"))?,
    )?;
    let declared =
        View::u32_le_at(body, at).ok_or_else(|| malformed("paramesh stream chunk is truncated"))?;
    if declared > MAX_STREAM_BYTES {
        return Err(malformed(format!(
            "paramesh stream declares {declared} bytes, above the {MAX_STREAM_BYTES}-byte limit"
        )));
    }
    let properties = body
        .get(at + 4..at + 6)
        .ok_or_else(|| malformed("paramesh stream chunk is truncated"))?;
    let payload = body
        .get(at + 6..)
        .ok_or_else(|| malformed("paramesh stream chunk is truncated"))?;
    if properties == [LZMA_PROPERTIES, RAW_STREAM_MODE] {
        if payload.len() != declared as usize {
            return Err(malformed(
                "raw paramesh stream length differs from its declared byte count",
            ));
        }
        return Ok(MeshStream {
            descriptor,
            bytes: payload.to_vec(),
        });
    }
    if properties != [LZMA_PROPERTIES, LZMA_DICTIONARY_LOG] {
        return Err(malformed("paramesh stream carries an undefined encoding"));
    }
    // `lzma-rs` reads the properties byte and the four-byte dictionary size
    // from the stream. The container stores a properties byte and a base-2
    // dictionary exponent.
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
    Ok(MeshStream {
        descriptor,
        bytes: out,
    })
}

/// A stream layout whose byte semantics this decoder implements.
#[derive(Clone, Copy)]
enum StreamLayout {
    /// One byte per element.
    Byte,
    /// A fixed number of unpacked f32 components per element.
    Float(u64),
    /// One octahedrally packed three-component direction per two f32 values.
    PackedDirection,
    /// One u32 value, with every nonterminal word interpreted as an i32 delta.
    TerminalDelta,
}

/// Require an exact descriptor before interpreting stream bytes.
fn require_layout(stream: &MeshStream, layout: StreamLayout) -> Result<(), CodecError> {
    let expected: &[(&str, StreamDescriptorValue)] = match layout {
        StreamLayout::Byte => &[("T", StreamDescriptorValue::Integer(0))],
        StreamLayout::Float(2) => &[
            ("D", StreamDescriptorValue::Integer(2)),
            ("T", StreamDescriptorValue::Integer(3)),
        ],
        StreamLayout::Float(3) => &[
            ("D", StreamDescriptorValue::Integer(3)),
            ("T", StreamDescriptorValue::Integer(3)),
        ],
        StreamLayout::Float(4) => &[
            ("D", StreamDescriptorValue::Integer(4)),
            ("T", StreamDescriptorValue::Integer(3)),
        ],
        StreamLayout::PackedDirection => &[
            ("D", StreamDescriptorValue::Integer(3)),
            ("T", StreamDescriptorValue::Integer(3)),
            ("U", StreamDescriptorValue::Boolean(true)),
        ],
        StreamLayout::TerminalDelta => &[
            ("T", StreamDescriptorValue::Integer(1)),
            ("d", StreamDescriptorValue::Integer(1)),
        ],
        StreamLayout::Float(_) => {
            return Err(malformed(
                "paramesh stream declares an unsupported f32 component count",
            ));
        }
    };
    if stream.descriptor.len() != expected.len()
        || expected.iter().any(|(expected_name, expected_value)| {
            !stream
                .descriptor
                .iter()
                .any(|(name, value)| name == expected_name && value == expected_value)
        })
    {
        return Err(malformed(
            "paramesh stream descriptor does not match its implemented layout",
        ));
    }
    Ok(())
}

/// Version 2 admits six exact stream descriptors and no other component type.
fn require_version_2_descriptor(stream: &MeshStream) -> Result<(), CodecError> {
    for layout in [
        StreamLayout::Byte,
        StreamLayout::Float(2),
        StreamLayout::Float(3),
        StreamLayout::Float(4),
        StreamLayout::PackedDirection,
        StreamLayout::TerminalDelta,
    ] {
        if require_layout(stream, layout).is_ok() {
            return Ok(());
        }
    }
    Err(malformed(
        "paramesh stream descriptor is outside the version-2 grammar",
    ))
}

/// Decode the concatenated `<Attrib>` XML fragments named by the registry.
fn attribute_names(
    stream: Option<&MeshStream>,
) -> Result<std::collections::BTreeMap<String, RegisteredAttributeName>, CodecError> {
    let Some(stream) = stream else {
        return Ok(std::collections::BTreeMap::new());
    };
    require_layout(stream, StreamLayout::Byte)?;
    let xml = std::str::from_utf8(&stream.bytes)
        .map_err(|_| malformed("paramesh attribute-name stream is not UTF-8"))?;
    let body = xml
        .strip_prefix("<?xml version=\"1.0\"?>")
        .ok_or_else(|| malformed("paramesh attribute-name stream has no XML declaration"))?;
    let wrapped = format!("<Root>{body}</Root>");
    let document = roxmltree::Document::parse(&wrapped)
        .map_err(|_| malformed("paramesh attribute-name stream is not XML"))?;
    let root = document.root_element();
    if root.tag_name().name() != "Root"
        || root.attributes().next().is_some()
        || root.children().any(|node| {
            !node.is_element()
                && (!node.is_text() || node.text().is_some_and(|text| !text.trim().is_empty()))
        })
    {
        return Err(malformed(
            "paramesh attribute-name stream has an invalid fragment envelope",
        ));
    }

    let mut names = std::collections::BTreeMap::new();
    for attribute in root.children().filter(roxmltree::Node::is_element) {
        if attribute.tag_name().name() != "Attrib"
            || attribute.attributes().next().is_some()
            || attribute.children().any(|node| {
                !node.is_element()
                    && (!node.is_text() || node.text().is_some_and(|text| !text.trim().is_empty()))
            })
        {
            return Err(malformed(
                "paramesh attribute-name stream has an undefined element",
            ));
        }
        let children = attribute
            .children()
            .filter(roxmltree::Node::is_element)
            .collect::<Vec<_>>();
        let [triangle_name, authored_name] = children.as_slice() else {
            return Err(malformed(
                "paramesh attribute-name record does not have two members",
            ));
        };
        if triangle_name.tag_name().name() != "TriName"
            || authored_name.tag_name().name() != "AmtName"
            || triangle_name.attributes().next().is_some()
            || authored_name.attributes().next().is_some()
        {
            return Err(malformed(
                "paramesh attribute-name record has an undefined member",
            ));
        }
        if triangle_name.children().any(|node| !node.is_text())
            || authored_name.children().any(|node| !node.is_text())
        {
            return Err(malformed(
                "paramesh attribute-name record member is not text",
            ));
        }
        let triangle_name = triangle_name
            .text()
            .ok_or_else(|| malformed("paramesh TriName is empty"))?;
        let authored_name = authored_name
            .text()
            .filter(|name| !name.is_empty())
            .ok_or_else(|| malformed("paramesh AmtName is empty"))?;
        if !triangle_name.is_ascii() {
            return Err(malformed("paramesh TriName is not ASCII"));
        }
        let guid_at = triangle_name
            .len()
            .checked_sub(36)
            .ok_or_else(|| malformed("paramesh TriName has no channel GUID"))?;
        let (prefix, resource_guid) = triangle_name.split_at(guid_at);
        let kind = match prefix {
            "color_tt" => AttributeNameKind::Color,
            "grp_tt" => AttributeNameKind::Group,
            "tco_tt" => AttributeNameKind::TextureCoordinate,
            _ => return Err(malformed("paramesh TriName has an undefined form")),
        };
        if !crate::bytes::is_guid_hyphenated(resource_guid) {
            return Err(malformed("paramesh TriName has an undefined form"));
        }
        if names
            .insert(
                resource_guid.to_ascii_uppercase(),
                RegisteredAttributeName {
                    kind,
                    authored_name: authored_name.to_owned(),
                },
            )
            .is_some()
        {
            return Err(malformed(
                "paramesh attribute-name stream repeats a channel GUID",
            ));
        }
    }
    if names.is_empty() {
        return Err(malformed(
            "paramesh attribute-name stream has no attribute records",
        ));
    }
    Ok(names)
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
            let component = View::f32_le_at(raw, 0).expect("chunks_exact(4)");
            if !component.is_finite() {
                return Err(malformed("paramesh vertex coordinate is not finite"));
            }
            *value = f64::from(component);
        }
        vertices.push(point);
    }
    Ok(vertices)
}

/// Resolve the delta-coded corner indices into triangles. The first corner is
/// implicit and is the unique starting index that keeps the complete corner
/// sequence inside the vertex domain. Every value before the last is the
/// two's-complement difference to the next corner. The final stored value does
/// not continue the sequence.
fn decode_triangles(stream: &[u8], vertices: usize) -> Result<Vec<[u32; 3]>, CodecError> {
    if !stream.len().is_multiple_of(4) || stream.is_empty() {
        return Err(malformed(
            "paramesh corner stream is not a whole number of values",
        ));
    }
    let values = stream.len() / 4;
    if !values.is_multiple_of(3) {
        return Err(malformed(
            "paramesh corner count is not a whole number of triangles",
        ));
    }
    let deltas = stream
        .chunks_exact(4)
        .take(values - 1)
        .map(|raw| i64::from(View::i32_le_at(raw, 0).expect("chunks_exact(4)")))
        .collect::<Vec<_>>();
    let mut relative = 0i64;
    let mut minimum = 0i64;
    let mut maximum = 0i64;
    for delta in &deltas {
        relative = relative
            .checked_add(*delta)
            .ok_or_else(|| malformed("paramesh corner delta accumulation overflows"))?;
        minimum = minimum.min(relative);
        maximum = maximum.max(relative);
    }
    let last_vertex = i64::try_from(vertices)
        .ok()
        .and_then(|count| count.checked_sub(1))
        .ok_or_else(|| malformed("paramesh corner stream has no vertex domain"))?;
    let lowest_start = minimum
        .checked_neg()
        .ok_or_else(|| malformed("paramesh corner start is out of range"))?;
    let highest_start = last_vertex
        .checked_sub(maximum)
        .ok_or_else(|| malformed("paramesh corner start is out of range"))?;
    if lowest_start != highest_start || lowest_start < 0 {
        return Err(malformed(
            "paramesh corner deltas do not determine one implicit starting index",
        ));
    }

    let mut current = lowest_start;
    let mut corners = Vec::with_capacity(values);
    corners.push(
        u32::try_from(current).map_err(|_| malformed("paramesh corner index is out of range"))?,
    );
    for delta in deltas {
        current += delta;
        let index = u32::try_from(current)
            .map_err(|_| malformed("paramesh corner index is out of range"))?;
        if usize::try_from(index).is_ok_and(|index| index >= vertices) {
            return Err(malformed("paramesh corner index names no vertex"));
        }
        corners.push(index);
    }
    Ok(corners
        .chunks_exact(3)
        .map(|corner| [corner[0], corner[1], corner[2]])
        .collect())
}

/// Decode terminal-delta framing into its complete value sequence.
///
/// Every word except the last is an i32 difference to the next value. The
/// final word is the absolute u32 value of the final element. The first value
/// is therefore the terminal minus the sum of the differences.
fn decode_terminal_delta_values(stream: &[u8]) -> Result<Vec<u32>, CodecError> {
    if !stream.len().is_multiple_of(4) {
        return Err(malformed(
            "paramesh terminal-delta stream is not a whole number of values",
        ));
    }
    if stream.is_empty() {
        return Ok(Vec::new());
    }

    let value_count = stream.len() / 4;
    let terminal_at = (value_count - 1) * 4;
    let terminal = i64::from(
        View::u32_le_at(stream, terminal_at)
            .expect("terminal word is inside a 4-byte-aligned stream"),
    );
    let mut delta_total = 0i64;
    for raw in stream[..terminal_at].chunks_exact(4) {
        let delta = i64::from(View::i32_le_at(raw, 0).expect("chunks_exact(4)"));
        delta_total = delta_total
            .checked_add(delta)
            .ok_or_else(|| malformed("paramesh terminal-delta accumulation overflows"))?;
    }
    let start = terminal
        .checked_sub(delta_total)
        .ok_or_else(|| malformed("paramesh terminal-delta start overflows"))?;
    let mut values = Vec::with_capacity(value_count);
    let mut current = start;
    values.push(
        u32::try_from(current)
            .map_err(|_| malformed("paramesh terminal-delta value is out of range"))?,
    );
    for raw in stream[..terminal_at].chunks_exact(4) {
        let delta = i64::from(View::i32_le_at(raw, 0).expect("chunks_exact(4)"));
        current = current
            .checked_add(delta)
            .ok_or_else(|| malformed("paramesh terminal-delta value overflows"))?;
        values.push(
            u32::try_from(current)
                .map_err(|_| malformed("paramesh terminal-delta value is out of range"))?,
        );
    }
    Ok(values)
}

/// Resolve an indexed channel's delta-coded corner positions.
///
/// The value stream starts with one default value for every vertex. The
/// remaining values are corner overrides in the order selected by this stream.
/// The first position is implicit. Each value before the final one is a
/// two's-complement difference from the previous position. The final value is
/// the absolute terminal position and does not continue the difference run.
fn decode_index_positions(
    stream: &[u8],
    value_count: u32,
    vertices: usize,
    corners: usize,
) -> Result<Vec<u32>, CodecError> {
    if !stream.len().is_multiple_of(4) {
        return Err(malformed(
            "paramesh channel index stream is not a whole number of values",
        ));
    }
    let vertex_count = u32::try_from(vertices)
        .map_err(|_| malformed("paramesh channel vertex count is out of range"))?;
    let override_count = value_count
        .checked_sub(vertex_count)
        .ok_or_else(|| malformed("paramesh indexed channel has fewer values than vertices"))?;
    let override_count_usize = usize::try_from(override_count)
        .map_err(|_| malformed("paramesh channel override count is out of range"))?;
    let expected_bytes = override_count_usize
        .checked_mul(4)
        .ok_or_else(|| malformed("paramesh channel index stream is too large"))?;
    if stream.len() != expected_bytes {
        return Err(malformed(
            "paramesh channel index count does not match its value count",
        ));
    }
    if override_count_usize == 0 {
        return Ok(Vec::new());
    }

    let decoded = decode_terminal_delta_values(stream)?;
    let mut positions = Vec::with_capacity(decoded.len());
    let mut previous = None;
    for position in decoded {
        if usize::try_from(position)
            .ok()
            .is_none_or(|position| position >= corners)
        {
            return Err(malformed(
                "paramesh channel index position names no triangle corner",
            ));
        }
        if previous.is_some_and(|previous| position <= previous) {
            return Err(malformed(
                "paramesh channel index positions are not strictly increasing",
            ));
        }
        positions.push(position);
        previous = Some(position);
    }
    Ok(positions)
}

/// Decode one octahedrally packed unit direction.
fn decode_packed_direction(packed: [f32; 2]) -> Result<[f64; 3], CodecError> {
    let [encoded_x, encoded_y] = packed.map(f64::from);
    if !encoded_x.is_finite()
        || !encoded_y.is_finite()
        || !(-1.0..=1.0).contains(&encoded_x)
        || !(-1.0..=1.0).contains(&encoded_y)
    {
        return Err(malformed(
            "paramesh packed direction is outside the octahedral domain",
        ));
    }

    let mut normal_x = encoded_x;
    let mut normal_y = encoded_y;
    let normal_z = 1.0 - normal_x.abs() - normal_y.abs();
    if normal_z < 0.0 {
        let unfolded_x = normal_x;
        normal_x = (1.0 - normal_y.abs()) * if unfolded_x >= 0.0 { 1.0 } else { -1.0 };
        normal_y = (1.0 - unfolded_x.abs()) * if normal_y >= 0.0 { 1.0 } else { -1.0 };
    }
    let length = (normal_x * normal_x + normal_y * normal_y + normal_z * normal_z).sqrt();
    if !length.is_finite() || length <= f64::EPSILON {
        return Err(malformed("paramesh packed direction is degenerate"));
    }
    Ok([normal_x / length, normal_y / length, normal_z / length])
}

/// Expand the role-0 packed-direction channel to one normal per triangle
/// corner. Indexed channels use their per-vertex defaults and corner overrides.
fn decode_corner_normals(
    attributes: &[MeshAttribute],
    vertices: usize,
    triangles: &[[u32; 3]],
) -> Result<Vec<[f64; 3]>, CodecError> {
    let mut channels = attributes.iter().filter(|attribute| {
        attribute.role == 0 && u64::from(attribute.element_code) == ELEMENT_PACKED_DIRECTION
    });
    let Some(attribute) = channels.next() else {
        return Ok(Vec::new());
    };
    if channels.next().is_some() {
        return Err(malformed(
            "paramesh registry declares more than one corner-normal channel",
        ));
    }
    if attribute.item_size != Some(PACKED_DIRECTION_BYTES)
        || !attribute
            .values
            .len()
            .is_multiple_of(PACKED_DIRECTION_BYTES as usize)
    {
        return Err(malformed(
            "paramesh corner-normal channel has no complete packed-direction table",
        ));
    }

    let mut table = Vec::with_capacity(attribute.values.len() / PACKED_DIRECTION_BYTES as usize);
    for raw in attribute
        .values
        .chunks_exact(PACKED_DIRECTION_BYTES as usize)
    {
        table.push(decode_packed_direction([
            View::f32_le_at(raw, 0).expect("packed-direction chunks_exact(8)"),
            View::f32_le_at(raw, 4).expect("packed-direction chunks_exact(8)"),
        ])?);
    }

    if attribute.domain == MeshAttributeDomain::Triangle {
        return Err(malformed(
            "paramesh role-0 packed directions do not address triangles",
        ));
    }
    let selectors = attribute
        .corner_selectors(vertices, triangles)
        .ok_or_else(|| malformed("paramesh corner-normal addressing is inconsistent"))?;

    selectors
        .into_iter()
        .map(|selector| {
            table
                .get(
                    usize::try_from(selector).map_err(|_| {
                        malformed("paramesh corner-normal selector is out of range")
                    })?,
                )
                .copied()
                .ok_or_else(|| malformed("paramesh corner-normal selector is out of range"))
        })
        .collect()
}

/// Decode registry field 7 as source-classified mesh edges.
fn registry_feature_edges(
    message: &[u8],
    name_table: &[(String, u64)],
    streams: &[MeshStream],
    triangles: &[[u32; 3]],
    vertices: usize,
) -> Result<Vec<[u32; 2]>, CodecError> {
    let mut declaration = None;
    for (field, value) in protobuf_fields(message)? {
        if field != REGISTRY_FEATURE_EDGES {
            continue;
        }
        let ProtobufValue::Bytes(entry) = value else {
            return Err(malformed(
                "paramesh feature-edge declaration is not a message",
            ));
        };
        if declaration.replace(entry).is_some() {
            return Err(malformed(
                "paramesh registry repeats its feature-edge declaration",
            ));
        }
    }
    let Some(entry) = declaration else {
        return Ok(Vec::new());
    };

    let mut stream_name = None;
    for (field, value) in protobuf_fields(entry)? {
        let (FEATURE_EDGE_STREAM, ProtobufValue::Bytes(name)) = (field, value) else {
            return Err(malformed(
                "paramesh feature-edge declaration has an undefined field",
            ));
        };
        let name = std::str::from_utf8(name)
            .map_err(|_| malformed("paramesh feature-edge stream name is not UTF-8"))?;
        if stream_name.replace(name).is_some() {
            return Err(malformed(
                "paramesh feature-edge declaration repeats its stream name",
            ));
        }
    }
    let stream_name = stream_name
        .ok_or_else(|| malformed("paramesh feature-edge declaration has no stream name"))?;
    let stream = name_table
        .iter()
        .position(|(name, _)| name == stream_name)
        .and_then(|position| streams.get(position))
        .ok_or_else(|| malformed("paramesh feature-edge declaration names no stream"))?;
    require_layout(stream, StreamLayout::TerminalDelta)?;
    let endpoints = decode_terminal_delta_values(&stream.bytes)?;
    if !endpoints.len().is_multiple_of(2) {
        return Err(malformed(
            "paramesh feature-edge stream has an unmatched endpoint",
        ));
    }

    let mut topology_edges = std::collections::BTreeSet::new();
    for [a, b, c] in triangles {
        for [left, right] in [[*a, *b], [*b, *c], [*c, *a]] {
            if left != right {
                topology_edges.insert([left.min(right), left.max(right)]);
            }
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut feature_edges = Vec::with_capacity(endpoints.len() / 2);
    for pair in endpoints.chunks_exact(2) {
        let edge = [pair[0], pair[1]];
        let high_in_domain = usize::try_from(edge[1]).is_ok_and(|endpoint| endpoint < vertices);
        if edge[0] >= edge[1] || !high_in_domain {
            return Err(malformed(
                "paramesh feature-edge endpoints are not an ascending vertex pair",
            ));
        }
        if !topology_edges.contains(&edge) {
            return Err(malformed(
                "paramesh feature-edge pair is not an edge of any triangle",
            ));
        }
        if !seen.insert(edge) {
            return Err(malformed("paramesh feature-edge stream repeats an edge"));
        }
        feature_edges.push(edge);
    }
    feature_edges.sort_unstable();
    Ok(feature_edges)
}

/// Decode one `.paramesh` container entry.
pub(crate) fn decode_mesh_container(bytes: &[u8]) -> Result<MeshContainer, CodecError> {
    if bytes.get(..MAGIC.len()) != Some(&MAGIC[..]) {
        return Err(malformed("paramesh container has no magic"));
    }
    match View::u32_le_at(bytes, MAGIC.len()) {
        Some(VERSION) => {}
        _ => return Err(malformed("paramesh container declares an unknown version")),
    }
    let protobuf_count = usize::try_from(
        View::u64_le_at(bytes, PROTOBUF_COUNT_AT)
            .ok_or_else(|| malformed("paramesh container is truncated"))?,
    )
    .map_err(|_| malformed("paramesh protobuf message is out of range"))?;
    let protobuf_end = PROTOBUF_AT
        .checked_add(protobuf_count)
        .ok_or_else(|| malformed("paramesh protobuf message is out of range"))?;
    let message = bytes
        .get(PROTOBUF_AT..protobuf_end)
        .ok_or_else(|| malformed("paramesh protobuf message is truncated"))?;
    let registry = mesh_registry(message)?;

    let mut at = protobuf_end;
    let mut name_table: Option<Vec<(String, u64)>> = None;
    let mut streams = Vec::new();
    while at < bytes.len() {
        let body_count = usize::try_from(
            View::u64_le_at(bytes, at)
                .ok_or_else(|| malformed("paramesh chunk header is truncated"))?,
        )
        .map_err(|_| malformed("paramesh chunk is out of range"))?;
        let kind = View::u32_le_at(bytes, at + 8)
            .ok_or_else(|| malformed("paramesh chunk header is truncated"))?;
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
    if name_table.len() != streams.len() {
        return Err(malformed(
            "paramesh name table and stream chunk counts differ",
        ));
    }
    for stream in &streams {
        require_version_2_descriptor(stream)?;
    }
    // The kind-4 chunks follow the name table in ascending stream-id order.
    name_table.sort_by_key(|(_, id)| *id);
    let named = |name: &str| {
        name_table
            .iter()
            .position(|(entry, _)| entry == name)
            .and_then(|position| streams.get(position))
    };
    let vertex_stream = named(&registry.vertex_stream)
        .ok_or_else(|| malformed("paramesh registry names no vertex stream"))?;
    require_layout(vertex_stream, StreamLayout::Float(3))?;
    let vertices = decode_vertices(&vertex_stream.bytes)?;
    let corner_stream = named(&registry.triangle_stream)
        .ok_or_else(|| malformed("paramesh registry names no triangle stream"))?;
    require_layout(corner_stream, StreamLayout::TerminalDelta)?;
    let triangles = decode_triangles(&corner_stream.bytes, vertices.len())?;
    let corner_count = triangles
        .len()
        .checked_mul(3)
        .ok_or_else(|| malformed("paramesh triangle corner count is out of range"))?;
    let attribute_name_stream = registry
        .attribute_name_stream
        .as_deref()
        .map(|name| {
            named(name).ok_or_else(|| malformed("paramesh attribute-name property names no stream"))
        })
        .transpose()?;
    let attribute_names = attribute_names(attribute_name_stream)?;
    let attributes = registry_attributes(
        message,
        &name_table,
        &streams,
        &attribute_names,
        vertices.len(),
        corner_count,
    )?;
    let feature_edges =
        registry_feature_edges(message, &name_table, &streams, &triangles, vertices.len())?;
    let corner_normals = decode_corner_normals(&attributes, vertices.len(), &triangles)?;
    let triangle_groups = registry_triangle_groups(&attributes, registry.face_group_count)?;
    let texture_ids = registry_texture_ids(&attributes)?;
    Ok(MeshContainer {
        fusion_uuid: registry.fusion_uuid,
        mesh_uuid: registry.mesh_uuid,
        vertices,
        triangles,
        feature_edges,
        corner_normals,
        triangle_groups,
        texture_ids,
        attributes,
    })
}

/// Collect registry-declared attribute channels in registry order.
///
/// The registry declares the channel; its named value and index streams supply
/// the data. Every declared stream must exist.
fn registry_attributes(
    message: &[u8],
    name_table: &[(String, u64)],
    streams: &[MeshStream],
    attribute_names: &std::collections::BTreeMap<String, RegisteredAttributeName>,
    vertices: usize,
    corners: usize,
) -> Result<Vec<MeshAttribute>, CodecError> {
    let named = |name: &str| {
        name_table
            .iter()
            .position(|(entry, _)| entry == name)
            .and_then(|position| streams.get(position))
    };
    let mut attributes = Vec::new();
    for (field, value) in protobuf_fields(message)? {
        let declared_domain = match field {
            REGISTRY_VERTEX_CHANNEL => MeshAttributeDomain::Vertex,
            REGISTRY_TRIANGLE_CHANNEL => MeshAttributeDomain::Triangle,
            _ => continue,
        };
        let ProtobufValue::Bytes(entry) = value else {
            return Err(malformed(
                "paramesh registry channel declaration is not a message",
            ));
        };
        let registration = registry_channel(entry, declared_domain)?;
        if declared_domain == MeshAttributeDomain::Triangle && registration.streams.index.is_some()
        {
            return Err(malformed(
                "paramesh triangle channel declares a corner index stream",
            ));
        }
        let stream = named(registration.streams.values)
            .ok_or_else(|| malformed("paramesh channel declares an absent value stream"))?;
        match registration.streams.element_code {
            ELEMENT_PAIR => require_layout(stream, StreamLayout::Float(2))?,
            ELEMENT_QUAD => require_layout(stream, StreamLayout::Float(4))?,
            ELEMENT_PACKED_DIRECTION => {
                require_layout(stream, StreamLayout::PackedDirection)?;
            }
            ELEMENT_TRIANGLE_DELTA => {
                if declared_domain != MeshAttributeDomain::Triangle {
                    return Err(malformed(
                        "paramesh delta-coded triangle elements use a non-triangle channel",
                    ));
                }
                require_layout(stream, StreamLayout::TerminalDelta)?;
            }
            _ => {}
        }
        let index_stream = registration
            .streams
            .index
            .map(|name| {
                named(name)
                    .ok_or_else(|| malformed("paramesh channel declares an absent index stream"))
            })
            .transpose()?;
        if let Some(index_stream) = index_stream {
            require_layout(index_stream, StreamLayout::TerminalDelta)?;
        }
        let registered_name = registration
            .resource_guid
            .as_ref()
            .and_then(|resource_guid| attribute_names.get(&resource_guid.to_ascii_uppercase()));
        if registered_name.is_some_and(|name| !match name.kind {
            AttributeNameKind::Color => {
                registration.role == 4 && registration.streams.element_code == ELEMENT_QUAD
            }
            AttributeNameKind::Group => {
                registration.domain == MeshAttributeDomain::Triangle
                    && registration.role == 1
                    && registration.streams.element_code == ELEMENT_TRIANGLE_DELTA
            }
            AttributeNameKind::TextureCoordinate => {
                registration.role == 3 && registration.streams.element_code == ELEMENT_PAIR
            }
        }) {
            return Err(malformed(
                "paramesh attribute-name prefix contradicts its channel declaration",
            ));
        }
        let authored_name = registered_name.map(|name| name.authored_name.clone());
        let mut attribute = MeshAttribute {
            role: registration.role,
            resource_guid: registration.resource_guid,
            authored_name,
            groups: registration.groups,
            element_code: u32::try_from(registration.streams.element_code)
                .map_err(|_| malformed("paramesh channel declares an out-of-range element code"))?,
            domain: registration.domain,
            item_size: element_bytes(registration.streams.element_code),
            values: stream.bytes.clone(),
            indices: None,
            triangle_values: None,
        };
        if attribute.item_size.is_some() && attribute.count().is_none() {
            return Err(malformed(
                "paramesh channel value stream ends inside an element",
            ));
        }
        if let (Some(index_stream), Some(count)) = (index_stream, attribute.count()) {
            attribute.indices = Some(decode_index_positions(
                &index_stream.bytes,
                count,
                vertices,
                corners,
            )?);
        }
        if attribute.item_size.is_some()
            && attribute.domain == MeshAttributeDomain::Vertex
            && attribute
                .count()
                .is_none_or(|count| usize::try_from(count) != Ok(vertices))
        {
            return Err(malformed(
                "paramesh vertex-channel element count differs from the vertex count",
            ));
        }
        if attribute.item_size.is_some()
            && attribute.domain == MeshAttributeDomain::Triangle
            && attribute
                .count()
                .is_none_or(|count| usize::try_from(count).ok() != corners.checked_div(3))
        {
            return Err(malformed(
                "paramesh triangle-channel element count differs from the triangle count",
            ));
        }
        if u64::from(attribute.element_code) == ELEMENT_TRIANGLE_DELTA {
            attribute.triangle_values = Some(decode_terminal_delta_values(&attribute.values)?);
        }
        attributes.push(attribute);
    }
    let mut resources = std::collections::BTreeSet::new();
    for attribute in &attributes {
        if let Some(resource_guid) = &attribute.resource_guid {
            if !resources.insert(resource_guid.to_ascii_uppercase()) {
                return Err(malformed(
                    "paramesh registry repeats a channel resource GUID",
                ));
            }
        }
    }
    if attribute_names
        .keys()
        .any(|resource_guid| !resources.contains(resource_guid))
    {
        return Err(malformed(
            "paramesh attribute-name stream names no registry channel",
        ));
    }
    Ok(attributes)
}

/// Project a role-0 code-7 channel as a triangle-group partition.
fn registry_triangle_groups(
    attributes: &[MeshAttribute],
    declared_count: u32,
) -> Result<Vec<MeshTriangleGroup>, CodecError> {
    let mut channels = attributes.iter().filter(|attribute| {
        attribute.domain == MeshAttributeDomain::Triangle
            && u64::from(attribute.element_code) == ELEMENT_TRIANGLE_DELTA
            && attribute.role == 0
    });
    let channel = channels.next();
    if channels.next().is_some() {
        return Err(malformed(
            "paramesh registry declares more than one face-group channel",
        ));
    }
    let declared_count = usize::try_from(declared_count)
        .map_err(|_| malformed("paramesh face-group count is out of range"))?;
    let Some(channel) = channel else {
        if declared_count == 0 {
            return Ok(Vec::new());
        }
        return Err(malformed(
            "paramesh registry has face groups but no face-group channel",
        ));
    };
    if channel.resource_guid.is_some()
        || channel.authored_name.is_some()
        || channel.groups.len() != declared_count
        || channel.groups.is_empty()
    {
        return Err(malformed(
            "paramesh face-group channel contradicts its registry declaration",
        ));
    }
    let values = channel
        .triangle_values
        .as_deref()
        .ok_or_else(|| malformed("paramesh face-group channel has no decoded values"))?;
    let mut memberships = channel
        .groups
        .iter()
        .map(|(key, group_guid)| (*key, group_guid.clone(), Vec::new()))
        .collect::<Vec<_>>();
    let group_indices = memberships
        .iter()
        .enumerate()
        .map(|(index, (key, _, _))| (*key, index))
        .collect::<std::collections::BTreeMap<_, _>>();
    for (triangle, key) in values.iter().enumerate() {
        let group_index = group_indices
            .get(key)
            .ok_or_else(|| malformed("paramesh triangle selects no face-group record"))?;
        memberships[*group_index].2.push(
            u32::try_from(triangle)
                .map_err(|_| malformed("paramesh triangle ordinal is out of range"))?,
        );
    }
    if memberships
        .iter()
        .any(|(_, _, triangles)| triangles.is_empty())
    {
        return Err(malformed(
            "paramesh face-group record has no triangle membership",
        ));
    }
    Ok(memberships
        .into_iter()
        .map(|(_, source_id, triangles)| MeshTriangleGroup {
            source_id,
            triangles,
        })
        .collect())
}

/// Decode the authored `tid` channel without resolving its Design texture table.
fn registry_texture_ids(attributes: &[MeshAttribute]) -> Result<Option<Vec<u32>>, CodecError> {
    let named = attributes
        .iter()
        .filter(|attribute| attribute.authored_name.as_deref() == Some("tid"))
        .collect::<Vec<_>>();
    let [channel] = named.as_slice() else {
        if named.is_empty() {
            return Ok(None);
        }
        return Err(malformed(
            "paramesh registry declares more than one tid channel",
        ));
    };
    if channel.domain != MeshAttributeDomain::Triangle
        || u64::from(channel.element_code) != ELEMENT_TRIANGLE_DELTA
        || channel.role != 1
        || channel.resource_guid.is_none()
        || !channel.groups.is_empty()
    {
        return Err(malformed(
            "paramesh tid channel has an invalid registry declaration",
        ));
    }
    channel
        .triangle_values
        .clone()
        .map(Some)
        .ok_or_else(|| malformed("paramesh tid channel has no decoded values"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One kind-4 chunk: the descriptor map, the uncompressed byte count, the
    /// two LZMA1 property bytes, and the raw LZMA1 stream.
    fn stream_chunk(descriptor: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut compressed = Vec::new();
        lzma_rs::lzma_compress(&mut std::io::Cursor::new(payload), &mut compressed)
            .expect("compress stream");
        let mut body = Vec::new();
        body.extend_from_slice(&(descriptor.len() as u16).to_le_bytes());
        body.extend_from_slice(descriptor);
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

    fn raw_stream_body(descriptor: &[u8], declared: u32, payload: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&(descriptor.len() as u16).to_le_bytes());
        body.extend_from_slice(descriptor);
        body.extend_from_slice(&declared.to_le_bytes());
        body.extend_from_slice(&[LZMA_PROPERTIES, RAW_STREAM_MODE]);
        body.extend_from_slice(payload);
        body
    }

    /// One container holding `v` and `t` in that stream-id order.
    fn container(guid: &str, vertices: &[f32], corners: &[i32]) -> Vec<u8> {
        let protobuf = registry_header(guid, 0);

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
        bytes.extend_from_slice(&stream_chunk(
            &[0x82, 0xa1, b'D', 3, 0xa1, b'T', 3],
            &vertex_stream,
        ));
        bytes.extend_from_slice(&stream_chunk(
            &[0x82, 0xa1, b'T', 1, 0xa1, b'd', 1],
            &corner_stream,
        ));
        bytes
    }

    const GUID: &str = "8a52d9b8-99b1-4a19-8409-c7c734298305";
    const MESH_GUID: &str = "f14c5fd0-4831-41dc-802d-145a4a5fb6bd";

    fn varint(mut value: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            let byte = u8::try_from(value & 0x7f).expect("seven bits");
            value >>= 7;
            if value == 0 {
                bytes.push(byte);
                return bytes;
            }
            bytes.push(byte | 0x80);
        }
    }

    fn bytes_field(field: u64, payload: &[u8]) -> Vec<u8> {
        let mut bytes = varint(field << 3 | 2);
        bytes.extend(varint(payload.len() as u64));
        bytes.extend_from_slice(payload);
        bytes
    }

    fn varint_field(field: u64, value: u64) -> Vec<u8> {
        let mut bytes = varint(field << 3);
        bytes.extend(varint(value));
        bytes
    }

    fn registry_header(fusion_uuid: &str, face_group_count: u32) -> Vec<u8> {
        let mut property = bytes_field(PROPERTY_KEY, b"fusion_uuid");
        property.extend(bytes_field(PROPERTY_TEXT, fusion_uuid.as_bytes()));
        let mut registry = bytes_field(REGISTRY_PROPERTY, &property);
        registry.extend(varint_field(
            REGISTRY_FACE_GROUP_COUNT,
            u64::from(face_group_count),
        ));
        registry.extend(bytes_field(REGISTRY_MESH_UUID, MESH_GUID.as_bytes()));
        registry.extend(bytes_field(REGISTRY_VERTICES, b"v"));
        registry.extend(bytes_field(REGISTRY_TRIANGLES, b"t"));
        registry
    }

    /// One registry channel entry: its role, element code, value-stream name,
    /// and index-stream name.
    fn channel_entry(
        field: u64,
        role: Option<u64>,
        element_code: u64,
        values: &str,
        index: Option<&str>,
    ) -> Vec<u8> {
        let mut streams = varint_field(STREAM_ELEMENT_CODE, element_code);
        streams.extend(bytes_field(STREAM_VALUES, values.as_bytes()));
        if let Some(index) = index {
            streams.extend(bytes_field(STREAM_INDEX, index.as_bytes()));
        }
        let mut entry = Vec::new();
        if let Some(role) = role {
            entry.extend(varint_field(CHANNEL_ROLE, role));
        }
        entry.extend(bytes_field(CHANNEL_STREAMS, &streams));
        bytes_field(field, &entry)
    }

    fn resource_channel_entry(
        field: u64,
        role: u64,
        resource_guid: &str,
        element_code: u64,
        values: &str,
    ) -> Vec<u8> {
        let mut streams = varint_field(STREAM_ELEMENT_CODE, element_code);
        streams.extend(bytes_field(STREAM_VALUES, values.as_bytes()));
        let mut entry = varint_field(CHANNEL_ROLE, role);
        entry.extend(bytes_field(CHANNEL_RESOURCE, resource_guid.as_bytes()));
        entry.extend(bytes_field(CHANNEL_STREAMS, &streams));
        bytes_field(field, &entry)
    }

    fn stream_property(key: &str, stream: &str) -> Vec<u8> {
        let mut property = bytes_field(PROPERTY_KEY, key.as_bytes());
        property.extend(bytes_field(PROPERTY_STREAM, stream.as_bytes()));
        bytes_field(REGISTRY_PROPERTY, &property)
    }

    fn face_group_channel_entry(values: &str, groups: &[(u32, &str)]) -> Vec<u8> {
        let mut streams = varint_field(STREAM_ELEMENT_CODE, ELEMENT_TRIANGLE_DELTA);
        streams.extend(bytes_field(STREAM_VALUES, values.as_bytes()));
        let mut entry = bytes_field(CHANNEL_STREAMS, &streams);
        for (key, group_guid) in groups {
            let mut group = varint_field(GROUP_KEY, u64::from(*key));
            group.extend(bytes_field(GROUP_GUID, group_guid.as_bytes()));
            entry.extend(bytes_field(CHANNEL_GROUP, &group));
        }
        bytes_field(REGISTRY_TRIANGLE_CHANNEL, &entry)
    }

    /// A descriptor map for a `count`-component `f32` element stream.
    fn float_descriptor(count: u8) -> Vec<u8> {
        vec![0x82, 0xa1, b'D', count, 0xa1, b'T', 3]
    }

    fn byte_descriptor() -> Vec<u8> {
        vec![0x81, 0xa1, b'T', 0]
    }

    /// A descriptor map for a delta-coded `i32` stream.
    fn delta_descriptor() -> Vec<u8> {
        vec![0x82, 0xa1, b'T', 1, 0xa1, b'd', 1]
    }

    /// A descriptor map for one octahedrally packed direction.
    fn packed_direction_descriptor() -> Vec<u8> {
        vec![0x83, 0xa1, b'D', 3, 0xa1, b'T', 3, 0xa1, b'U', 0xc3]
    }

    fn feature_edge_entry(stream: &str) -> Vec<u8> {
        bytes_field(
            REGISTRY_FEATURE_EDGES,
            &bytes_field(FEATURE_EDGE_STREAM, stream.as_bytes()),
        )
    }

    /// Encode a complete value sequence with terminal-delta framing.
    fn terminal_delta_values(values: &[i32]) -> Vec<u8> {
        if values.is_empty() {
            return Vec::new();
        }
        let mut encoded = Vec::with_capacity(values.len() * 4);
        for pair in values.windows(2) {
            encoded.extend_from_slice(&(pair[1] - pair[0]).to_le_bytes());
        }
        encoded.extend_from_slice(
            &u32::try_from(*values.last().expect("terminal value"))
                .expect("nonnegative terminal value")
                .to_le_bytes(),
        );
        encoded
    }

    fn packed_directions(values: &[[f32; 2]]) -> Vec<u8> {
        values
            .iter()
            .flatten()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    fn assert_malformed(result: Result<MeshContainer, CodecError>) {
        match result {
            Err(CodecError::Malformed(_)) => {}
            Err(error) => panic!("expected malformed error, got {error:?}"),
            Ok(_) => panic!("expected malformed error, got a mesh"),
        }
    }

    fn replace_unique(bytes: &mut [u8], from: &[u8], to: &[u8]) {
        assert_eq!(from.len(), to.len());
        let offsets = bytes
            .windows(from.len())
            .enumerate()
            .filter_map(|(offset, window)| (window == from).then_some(offset))
            .collect::<Vec<_>>();
        let [offset] = offsets.as_slice() else {
            panic!("expected one replacement site, got {}", offsets.len());
        };
        bytes[*offset..*offset + to.len()].copy_from_slice(to);
    }

    /// One container holding `v`, `t`, and every extra named stream in
    /// stream-id order, with `registry` appended to the protobuf message.
    fn container_with_registry(
        vertices: &[f32],
        corners: &[i32],
        registry: &[u8],
        extra: &[(&str, Vec<u8>, Vec<u8>)],
    ) -> Vec<u8> {
        container_with_registry_and_groups(vertices, corners, registry, extra, 0)
    }

    fn container_with_registry_and_groups(
        vertices: &[f32],
        corners: &[i32],
        registry: &[u8],
        extra: &[(&str, Vec<u8>, Vec<u8>)],
        face_group_count: u32,
    ) -> Vec<u8> {
        let mut protobuf = registry_header(GUID, face_group_count);
        protobuf.extend_from_slice(registry);

        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&[0; 32]);
        bytes.extend_from_slice(&(protobuf.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&protobuf);

        // The name table maps every stream name to its id; `v` and `t` take
        // ids 2 and 3, and each extra stream follows in declaration order.
        let mut table = vec![0x80 | u8::try_from(2 + extra.len()).expect("stream count")];
        for (index, name) in [("v", 2u8), ("t", 3)]
            .iter()
            .map(|(name, id)| (*name, *id))
            .chain(
                extra
                    .iter()
                    .enumerate()
                    .map(|(index, (name, _, _))| (*name, u8::try_from(4 + index).expect("id"))),
            )
            .map(|(name, id)| (id, name))
        {
            table.push(0xa0 | u8::try_from(name.len()).expect("short name"));
            table.extend_from_slice(name.as_bytes());
            table.push(index);
        }
        let mut name_chunk = (table.len() as u64).to_le_bytes().to_vec();
        name_chunk.extend_from_slice(&CHUNK_NAME_TABLE.to_le_bytes());
        name_chunk.append(&mut table);
        bytes.extend_from_slice(&name_chunk);

        bytes.extend_from_slice(&stream_chunk(
            &float_descriptor(3),
            &vertices
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>(),
        ));
        bytes.extend_from_slice(&stream_chunk(
            &delta_descriptor(),
            &corners
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>(),
        ));
        for (_, descriptor, payload) in extra {
            bytes.extend_from_slice(&stream_chunk(descriptor, payload));
        }
        bytes
    }

    /// Three vertices and one triangle, the smallest mesh a channel can
    /// address.
    const TRIANGLE_VERTICES: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0];
    const TRIANGLE_CORNERS: [i32; 3] = [1, 1, 7];
    const TWO_TRIANGLE_VERTICES: [f32; 12] =
        [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0];
    const TWO_TRIANGLE_CORNERS: [i32; 6] = [1, 1, 1, -2, 1, 7];

    /// A channel with no index stream stores one value per vertex, so its
    /// element width and count are settled and it transfers.
    #[test]
    fn vertex_domain_channel_carries_one_element_per_vertex() {
        let uv = (0..3)
            .flat_map(|index| [index as f32, 0.5])
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let registry = channel_entry(REGISTRY_VERTEX_CHANNEL, Some(3), ELEMENT_PAIR, "r0", None);
        let mesh = decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[("r0", float_descriptor(2), uv.clone())],
        ))
        .expect("mesh container");
        let attribute = &mesh.attributes[0];
        assert_eq!(attribute.domain, MeshAttributeDomain::Vertex);
        assert_eq!(attribute.role, 3);
        assert_eq!(attribute.item_size, Some(8));
        assert_eq!(attribute.count(), Some(3));
        assert_eq!(attribute.values, uv);
    }

    /// A channel that declares an index stream addresses triangle corners, so
    /// its value count is independent of the vertex count.
    #[test]
    fn index_stream_makes_a_channel_corner_domain() {
        let colors = (0..5)
            .flat_map(|index| [index as f32, 0.0, 0.0, 1.0])
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let registry = channel_entry(
            REGISTRY_VERTEX_CHANNEL,
            Some(4),
            ELEMENT_QUAD,
            "r0",
            Some("r0i"),
        );
        let mesh = decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[
                ("r0", float_descriptor(4), colors),
                (
                    "r0i",
                    delta_descriptor(),
                    [1i32, 1].into_iter().flat_map(i32::to_le_bytes).collect(),
                ),
            ],
        ))
        .expect("mesh container");
        let attribute = &mesh.attributes[0];
        assert_eq!(attribute.domain, MeshAttributeDomain::Corner);
        assert_eq!(attribute.item_size, Some(16));
        assert_eq!(attribute.count(), Some(5));
        assert_eq!(attribute.indices, Some(vec![0, 1]));
    }

    #[test]
    fn indexed_positions_use_the_absolute_terminal_to_resolve_the_start() {
        let bytes = [1i32, 1, 2, 1, 9]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            decode_index_positions(&bytes, 8, 3, 10).expect("indexed positions"),
            [4, 5, 6, 8, 9]
        );
    }

    #[test]
    fn indexed_positions_reject_duplicate_or_out_of_range_corners() {
        let duplicate = [0i32, 0]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>();
        assert!(decode_index_positions(&duplicate, 5, 3, 3).is_err());

        let out_of_range = [4i32]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>();
        assert!(decode_index_positions(&out_of_range, 4, 3, 3).is_err());
    }

    #[test]
    fn octahedral_directions_cover_axes_and_the_negative_hemisphere() {
        let cases = [
            ([0.0, 0.0], [0.0, 0.0, 1.0]),
            ([1.0, 0.0], [1.0, 0.0, 0.0]),
            ([0.0, -1.0], [0.0, -1.0, 0.0]),
            ([1.0, 1.0], [0.0, 0.0, -1.0]),
            (
                [-0.75, 0.75],
                [
                    -0.408_248_290_463_863_1,
                    0.408_248_290_463_863_1,
                    -0.816_496_580_927_726_1,
                ],
            ),
        ];
        for (packed, expected) in cases {
            let actual = decode_packed_direction(packed).expect("packed direction");
            for (actual, expected) in actual.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 1.0e-12);
            }
        }
    }

    #[test]
    fn indexed_packed_directions_expand_vertex_defaults_and_corner_overrides() {
        let registry = channel_entry(
            REGISTRY_VERTEX_CHANNEL,
            Some(0),
            ELEMENT_PACKED_DIRECTION,
            "r0",
            Some("r0i"),
        );
        let mesh = decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[
                (
                    "r0",
                    packed_direction_descriptor(),
                    packed_directions(&[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]),
                ),
                ("r0i", delta_descriptor(), terminal_delta_values(&[1])),
            ],
        ))
        .expect("mesh container");
        assert_eq!(
            mesh.corner_normals,
            [[0.0, 0.0, 1.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]]
        );
        assert_eq!(mesh.attributes[0].domain, MeshAttributeDomain::Corner);
        assert_eq!(mesh.attributes[0].indices, Some(vec![1]));
    }

    #[test]
    fn packed_directions_require_the_octahedral_descriptor_and_domain() {
        let registry = channel_entry(
            REGISTRY_VERTEX_CHANNEL,
            Some(0),
            ELEMENT_PACKED_DIRECTION,
            "r0",
            None,
        );
        let valid = packed_directions(&[[0.0, 0.0]; 3]);
        assert_malformed(decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[("r0", float_descriptor(3), valid)],
        )));

        for invalid in [[f32::NAN, 0.0], [1.000_001, 0.0]] {
            let mut values = [[0.0, 0.0]; 3];
            values[0] = invalid;
            assert_malformed(decode_mesh_container(&container_with_registry(
                &TRIANGLE_VERTICES,
                &TRIANGLE_CORNERS,
                &registry,
                &[(
                    "r0",
                    packed_direction_descriptor(),
                    packed_directions(&values),
                )],
            )));
        }

        let registry = channel_entry(
            REGISTRY_TRIANGLE_CHANNEL,
            Some(0),
            ELEMENT_PACKED_DIRECTION,
            "r0",
            None,
        );
        assert_malformed(decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[(
                "r0",
                packed_direction_descriptor(),
                packed_directions(&[[0.0, 0.0]]),
            )],
        )));
    }

    #[test]
    fn feature_edges_use_terminal_deltas_and_canonical_ir_order() {
        let registry = feature_edge_entry("edges");
        let mesh = decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[(
                "edges",
                delta_descriptor(),
                terminal_delta_values(&[1, 2, 0, 1]),
            )],
        ))
        .expect("mesh container");
        assert_eq!(mesh.feature_edges, [[0, 1], [1, 2]]);
    }

    #[test]
    fn an_empty_feature_edge_stream_is_an_empty_classification() {
        let registry = feature_edge_entry("edges");
        let mesh = decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[("edges", delta_descriptor(), Vec::new())],
        ))
        .expect("mesh container");
        assert!(mesh.feature_edges.is_empty());
    }

    #[test]
    fn feature_edges_must_be_complete_unique_topology_edges() {
        let registry = feature_edge_entry("edges");
        for endpoints in [
            vec![0, 1, 2],
            vec![1, 0],
            vec![1, 1],
            vec![0, 3],
            vec![0, 1, 0, 1],
        ] {
            assert_malformed(decode_mesh_container(&container_with_registry(
                &TRIANGLE_VERTICES,
                &TRIANGLE_CORNERS,
                &registry,
                &[(
                    "edges",
                    delta_descriptor(),
                    terminal_delta_values(&endpoints),
                )],
            )));
        }

        let four_vertices = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 2.0, 2.0, 0.0];
        assert_malformed(decode_mesh_container(&container_with_registry(
            &four_vertices,
            &TRIANGLE_CORNERS,
            &registry,
            &[("edges", delta_descriptor(), terminal_delta_values(&[0, 3]))],
        )));
    }

    #[test]
    fn feature_edge_declarations_require_one_existing_delta_stream() {
        let registry = feature_edge_entry("absent");
        assert_malformed(decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[],
        )));

        let registry = [feature_edge_entry("edges"), feature_edge_entry("edges")].concat();
        assert_malformed(decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[("edges", delta_descriptor(), Vec::new())],
        )));

        let registry = feature_edge_entry("edges");
        assert_malformed(decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[("edges", float_descriptor(2), Vec::new())],
        )));
    }

    /// The role-0 code-7 channel maps each triangle to one face-group record.
    #[test]
    fn triangle_domain_code7_channel_decodes_face_groups() {
        const GROUP: &str = "d46db2ae-2a8b-4624-b83c-bcb6cbce247f";
        let registry = face_group_channel_entry("r0", &[(0, GROUP)]);
        let mesh = decode_mesh_container(&container_with_registry_and_groups(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[("r0", delta_descriptor(), vec![0; 4])],
            1,
        ))
        .expect("mesh container");
        let attribute = &mesh.attributes[0];
        assert_eq!(attribute.domain, MeshAttributeDomain::Triangle);
        assert_eq!(attribute.element_code, 7);
        assert_eq!(attribute.item_size, Some(4));
        assert_eq!(attribute.count(), Some(1));
        assert_eq!(attribute.triangle_values, Some(vec![0]));
        assert_eq!(mesh.triangle_groups.len(), 1);
        assert_eq!(mesh.triangle_groups[0].source_id, GROUP);
        assert_eq!(mesh.triangle_groups[0].triangles, [0]);
    }

    #[test]
    fn face_group_count_and_keys_bound_the_triangle_partition() {
        const GROUP_A: &str = "d46db2ae-2a8b-4624-b83c-bcb6cbce247f";
        const GROUP_B: &str = "7619477f-0f38-4b32-860e-81c9feb3d0f4";
        let registry = face_group_channel_entry("r0", &[(0, GROUP_A), (1, GROUP_B)]);
        let valid = container_with_registry_and_groups(
            &TWO_TRIANGLE_VERTICES,
            &TWO_TRIANGLE_CORNERS,
            &registry,
            &[("r0", delta_descriptor(), terminal_delta_values(&[0, 1]))],
            2,
        );
        let mesh = decode_mesh_container(&valid).expect("two face groups");
        assert_eq!(mesh.mesh_uuid, MESH_GUID);
        assert_eq!(mesh.triangle_groups.len(), 2);
        assert_eq!(mesh.triangle_groups[0].triangles, [0]);
        assert_eq!(mesh.triangle_groups[1].triangles, [1]);

        assert_malformed(decode_mesh_container(&container_with_registry_and_groups(
            &TWO_TRIANGLE_VERTICES,
            &TWO_TRIANGLE_CORNERS,
            &registry,
            &[("r0", delta_descriptor(), terminal_delta_values(&[0, 1]))],
            1,
        )));
        assert_malformed(decode_mesh_container(&container_with_registry_and_groups(
            &TWO_TRIANGLE_VERTICES,
            &TWO_TRIANGLE_CORNERS,
            &registry,
            &[("r0", delta_descriptor(), terminal_delta_values(&[0, 2]))],
            2,
        )));
    }

    #[test]
    fn authored_tid_channel_decodes_triangle_texture_ids() {
        const RESOURCE: &str = "2060d08b-7434-4786-9506-b0ee951bb9dc";
        let mut registry = resource_channel_entry(
            REGISTRY_TRIANGLE_CHANNEL,
            1,
            RESOURCE,
            ELEMENT_TRIANGLE_DELTA,
            "r0",
        );
        registry.extend(stream_property("attname.amt.autodesk", "names"));
        let xml = format!(
            "<?xml version=\"1.0\"?>\n<Attrib>\n<TriName>grp_tt{RESOURCE}</TriName>\n\
             <AmtName>tid</AmtName>\n</Attrib>\n"
        );
        let mesh = decode_mesh_container(&container_with_registry(
            &TWO_TRIANGLE_VERTICES,
            &TWO_TRIANGLE_CORNERS,
            &registry,
            &[
                ("r0", delta_descriptor(), terminal_delta_values(&[0, 2])),
                ("names", byte_descriptor(), xml.into_bytes()),
            ],
        ))
        .expect("texture ids");
        assert_eq!(mesh.texture_ids, Some(vec![0, 2]));
        assert_eq!(mesh.attributes[0].resource_guid.as_deref(), Some(RESOURCE));
        assert_eq!(mesh.attributes[0].authored_name.as_deref(), Some("tid"));

        let invalid_xml = format!(
            "<?xml version=\"1.0\"?>\n<Attrib>\n<TriName>color_tt{RESOURCE}</TriName>\n\
             <AmtName>tid</AmtName>\n</Attrib>\n"
        );
        assert_malformed(decode_mesh_container(&container_with_registry(
            &TWO_TRIANGLE_VERTICES,
            &TWO_TRIANGLE_CORNERS,
            &registry,
            &[
                ("r0", delta_descriptor(), terminal_delta_values(&[0, 2])),
                ("names", byte_descriptor(), invalid_xml.into_bytes()),
            ],
        )));

        for invalid_xml in [
            format!(
                "<?xml version=\"1.0\"?><Attrib><TriName>é{RESOURCE}</TriName>\
                 <AmtName>tid</AmtName></Attrib>"
            ),
            format!(
                "<?xml version=\"1.0\"?><Attrib><TriName>grp_tt{RESOURCE}<Extra/></TriName>\
                 <AmtName>tid</AmtName></Attrib>"
            ),
        ] {
            assert_malformed(decode_mesh_container(&container_with_registry(
                &TWO_TRIANGLE_VERTICES,
                &TWO_TRIANGLE_CORNERS,
                &registry,
                &[
                    ("r0", delta_descriptor(), terminal_delta_values(&[0, 2])),
                    ("names", byte_descriptor(), invalid_xml.into_bytes()),
                ],
            )));
        }
    }

    /// A known vertex-domain channel must carry exactly one value per vertex.
    #[test]
    fn vertex_domain_channel_rejects_a_contradicted_count() {
        let registry = channel_entry(REGISTRY_VERTEX_CHANNEL, Some(3), ELEMENT_PAIR, "r0", None);
        assert_malformed(decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[("r0", float_descriptor(2), vec![0; 8 * 4])],
        )));
    }

    /// The registry declares the channel; its value stream supplies the data.
    /// A missing declared value stream makes the container malformed.
    #[test]
    fn a_channel_naming_an_absent_stream_is_rejected() {
        let registry = channel_entry(
            REGISTRY_VERTEX_CHANNEL,
            Some(3),
            ELEMENT_PAIR,
            "absent",
            None,
        );
        assert_malformed(decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[],
        )));

        let registry = channel_entry(
            REGISTRY_VERTEX_CHANNEL,
            Some(4),
            ELEMENT_QUAD,
            "r0",
            Some("absent"),
        );
        assert_malformed(decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[("r0", float_descriptor(4), vec![0; 3 * 16])],
        )));
    }

    #[test]
    fn malformed_protobuf_cannot_silently_end_the_registry_walk() {
        assert!(matches!(
            protobuf_fields(&[0x80]),
            Err(CodecError::Malformed(_))
        ));
        assert!(matches!(
            protobuf_fields(&[0x0b]),
            Err(CodecError::Malformed(_))
        ));

        let registry = varint_field(REGISTRY_VERTEX_CHANNEL, 0);
        assert_malformed(decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &registry,
            &[],
        )));

        let undefined = varint_field(10, 1);
        assert_malformed(decode_mesh_container(&container_with_registry(
            &TRIANGLE_VERTICES,
            &TRIANGLE_CORNERS,
            &undefined,
            &[],
        )));
    }

    #[test]
    fn name_table_requires_unique_complete_stream_bindings() {
        for table in [
            vec![0x82, 0xa1, b'v', 2, 0xa1, b'v', 3],
            vec![0x82, 0xa1, b'v', 2, 0xa1, b't', 2],
            vec![0x81, 0xa1, b'v', 2, 0],
            vec![0x81, 0xa0, 2],
        ] {
            assert!(matches!(
                message_pack_name_table(&table),
                Err(CodecError::Malformed(_))
            ));
        }
    }

    /// Two triangles over four vertices. The delta range uniquely determines
    /// implicit starting corner zero, and the terminal value is not a corner.
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

    #[test]
    fn registry_names_the_core_geometry_streams() {
        let mut bytes = container(GUID, &TRIANGLE_VERTICES, &TRIANGLE_CORNERS);
        replace_unique(
            &mut bytes,
            &bytes_field(REGISTRY_VERTICES, b"v"),
            &bytes_field(REGISTRY_VERTICES, b"p"),
        );
        replace_unique(
            &mut bytes,
            &bytes_field(REGISTRY_TRIANGLES, b"t"),
            &bytes_field(REGISTRY_TRIANGLES, b"i"),
        );
        replace_unique(&mut bytes, &[0xa1, b'v', 2], &[0xa1, b'p', 2]);
        replace_unique(&mut bytes, &[0xa1, b't', 3], &[0xa1, b'i', 3]);

        let mesh = decode_mesh_container(&bytes).expect("renamed core streams");
        assert_eq!(mesh.fusion_uuid, GUID);
        assert_eq!(mesh.mesh_uuid, MESH_GUID);
        assert_eq!(mesh.triangles, [[0, 1, 2]]);
    }

    #[test]
    fn mesh_uuid_is_lowercase_version_4_with_an_rfc_4122_variant() {
        for invalid in [
            MESH_GUID.to_ascii_uppercase(),
            "f14c5fd0-4831-31dc-802d-145a4a5fb6bd".into(),
            "f14c5fd0-4831-41dc-702d-145a4a5fb6bd".into(),
        ] {
            let mut bytes = container(GUID, &TRIANGLE_VERTICES, &TRIANGLE_CORNERS);
            replace_unique(&mut bytes, MESH_GUID.as_bytes(), invalid.as_bytes());
            assert_malformed(decode_mesh_container(&bytes));
        }
    }

    #[test]
    fn corner_delta_bounds_determine_nonzero_implicit_start() {
        let triangles = decode_triangles(
            &[
                -4i32, 1, 3, -3, 1, 2, -2, 1, 1, -1, -3, 5, -4, -1, 5, -3, -1, 4, -2, -1, 3, -5, 3,
                3,
            ]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect::<Vec<_>>(),
            6,
        )
        .expect("bounded corner deltas");
        assert_eq!(
            triangles,
            [
                [4, 0, 1],
                [4, 1, 2],
                [4, 2, 3],
                [4, 3, 0],
                [5, 1, 0],
                [5, 2, 1],
                [5, 3, 2],
                [5, 0, 3],
            ]
        );
    }

    #[test]
    fn odd_fan_delta_bounds_determine_zero_implicit_start() {
        let triangles = decode_triangles(
            &[1i32, 1, -2, 2, 1, -3, 3, 1, -4, 4, 1, -5, 5, -4, 1]
                .into_iter()
                .flat_map(i32::to_le_bytes)
                .collect::<Vec<_>>(),
            6,
        )
        .expect("odd fan deltas");
        assert_eq!(
            triangles,
            [[0, 1, 2], [0, 2, 3], [0, 3, 4], [0, 4, 5], [0, 5, 1]]
        );
    }

    /// The decoder rejects a corner run that ends before a complete triangle.
    #[test]
    fn container_refuses_a_partial_triangle() {
        let container = container(
            GUID,
            &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
            &[1, 1, 1, 7],
        );
        assert!(decode_mesh_container(&container).is_err());
    }

    /// The decoder rejects a corner index outside the vertex stream.
    #[test]
    fn container_refuses_a_corner_index_beyond_the_vertex_stream() {
        let container = container(
            GUID,
            &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
            &[1, 9, 1, 7],
        );
        assert!(decode_mesh_container(&container).is_err());
    }

    /// The decoder rejects a vertex descriptor with an unsupported component
    /// type.
    #[test]
    fn container_refuses_a_vertex_descriptor_with_the_wrong_component_type() {
        let mut container = container(GUID, &[0.0, 0.0, 0.0], &[0, 0]);
        let descriptor = [0x82, 0xa1, b'D', 3, 0xa1, b'T', 3];
        let at = container
            .windows(descriptor.len())
            .position(|window| window == descriptor)
            .expect("vertex descriptor");
        container[at + 6] = 1;
        assert!(decode_mesh_container(&container).is_err());
    }

    /// Corner bytes are not treated as deltas unless their descriptor says
    /// that they are delta-coded.
    #[test]
    fn container_refuses_a_corner_descriptor_without_delta_coding() {
        let mut container = container(GUID, &[0.0, 0.0, 0.0], &[0, 0]);
        let descriptor = [0x82, 0xa1, b'T', 1, 0xa1, b'd', 1];
        let at = container
            .windows(descriptor.len())
            .position(|window| window == descriptor)
            .expect("corner descriptor");
        container[at + 6] = 0;
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

    #[test]
    fn raw_stream_requires_its_exact_declared_byte_count() {
        let descriptor = [0x82, 0xa1, b'T', 1, 0xa1, b'd', 1];
        let payload = [1, 2, 3, 4];
        let stream =
            inflate_stream(&raw_stream_body(&descriptor, 4, &payload)).expect("exact raw stream");
        assert_eq!(stream.bytes, payload);
        assert!(inflate_stream(&raw_stream_body(&descriptor, 3, &payload)).is_err());
        assert!(inflate_stream(&raw_stream_body(&descriptor, 5, &payload)).is_err());
    }

    #[test]
    fn stream_descriptor_retains_boolean_values() {
        assert_eq!(
            stream_descriptor(&[0x83, 0xa1, b'D', 3, 0xa1, b'T', 3, 0xa1, b'U', 0xc3])
                .expect("descriptor"),
            vec![
                ("D".into(), StreamDescriptorValue::Integer(3)),
                ("T".into(), StreamDescriptorValue::Integer(3)),
                ("U".into(), StreamDescriptorValue::Boolean(true)),
            ]
        );
    }

    #[test]
    fn version_2_descriptor_component_types_form_a_closed_enum() {
        for descriptor in [
            byte_descriptor(),
            delta_descriptor(),
            float_descriptor(2),
            float_descriptor(3),
            float_descriptor(4),
            packed_direction_descriptor(),
        ] {
            let stream = MeshStream {
                descriptor: stream_descriptor(&descriptor).expect("version-2 descriptor"),
                bytes: Vec::new(),
            };
            assert!(require_version_2_descriptor(&stream).is_ok());
        }
        for descriptor in [
            vec![0x81, 0xa1, b'T', 2],
            vec![0x81, 0xa1, b'T', 4],
            vec![0x81, 0xa1, b'T', 0xcc, u8::MAX],
        ] {
            let stream = MeshStream {
                descriptor: stream_descriptor(&descriptor).expect("scalar descriptor"),
                bytes: Vec::new(),
            };
            assert!(matches!(
                require_version_2_descriptor(&stream),
                Err(CodecError::Malformed(_))
            ));
        }
    }
}
