// SPDX-License-Identifier: Apache-2.0
//! Schema-driven decoding of Protein `InstanceProperties` records.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Cursor, Read};

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

/// Neutral material and texture projection.
pub mod appearance;

/// Paged logical-record framing.
pub mod framing;

/// Decoded property carriers and their serialized representation.
pub mod property;
use property::{DecodedProperty, PropertyContent, PropertyValue};

/// Byte-offset constants generated from `docs/layouts/protein.toml`.
mod layout;
use layout::{continuation_page, record_start_page, terminal_page};

/// Instance-stream header length in bytes.
pub const STREAM_HEADER_LEN: usize = layout::instance_stream_header::LEN;
/// Instance-page length in bytes (`0x88`).
pub const PAGE_SIZE: usize = layout::record_start_page::LEN;
/// Record-start marker at page bytes 4..8.
pub const RECORD_MARKER: &[u8] = &record_start_page::MARKER_VALUE;
/// Continuation marker at page bytes 4..8.
pub const CONTINUATION_MARKER: &[u8] = &continuation_page::MARKER_VALUE;
/// Terminal marker at page bytes 0..4.
pub const TERMINAL_MARKER: &[u8] = &terminal_page::MARKER_VALUE;
const MAX_SCHEMA_BYTES: u64 = 128 * 1024 * 1024;
const XML_NODE_RESERVATION_BYTES: u64 = 192;
const XML_ATTRIBUTE_RESERVATION_BYTES: u64 = 192;

fn take_lp_utf8_capped(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    at: &mut usize,
    max: usize,
) -> Result<Option<String>, CodecError> {
    let mut view = View::over_retained(bytes);
    let Some(()) = view.seek(*at) else {
        return Ok(None);
    };
    let Some(count) = view.u32_le().and_then(|count| usize::try_from(count).ok()) else {
        return Ok(None);
    };
    let Some(next) = at.checked_add(4) else {
        return Ok(None);
    };
    *at = next;
    if count > max {
        return Ok(None);
    }
    let Some(end) = at.checked_add(count) else {
        return Ok(None);
    };
    let Some(value) = bytes
        .get(*at..end)
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
    else {
        return Ok(None);
    };
    if let Some(ctx) = ctx {
        ctx.charge_retained(count as u64, "Protein decoded string")?;
    }
    let value = value.to_owned();
    *at = end;
    Ok(Some(value))
}

fn read_entry_bounded(
    entry: &mut impl Read,
    declared_size: u64,
    name: &str,
) -> Result<Vec<u8>, CodecError> {
    if declared_size > MAX_SCHEMA_BYTES {
        return Err(CodecError::malformed(format_args!(
            "Protein schema {name} exceeds the {MAX_SCHEMA_BYTES}-byte limit"
        )));
    }
    let mut bytes = Vec::new();
    let mut limited = entry.take(MAX_SCHEMA_BYTES + 1);
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let read = limited.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        bytes.try_reserve(read).map_err(|_| {
            cadmpeg_core::decode::refuse_local_limit(
                "Protein schema allocation",
                MAX_SCHEMA_BYTES,
                bytes.len().saturating_add(read) as u64,
            )
        })?;
        bytes.extend_from_slice(&chunk[..read]);
    }
    if bytes.len() as u64 > MAX_SCHEMA_BYTES {
        return Err(CodecError::malformed(format_args!(
            "Protein schema {name} exceeds the {MAX_SCHEMA_BYTES}-byte limit"
        )));
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ValueCarrier {
    Boolean,
    Integer,
    Float,
    UnitFloat,
    Distance,
    String,
    Color,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ValueLayout {
    Single(ValueCarrier),
    Multiple(ValueCarrier),
    TextureUri,
}

#[derive(Clone, Debug)]
enum Property {
    Reference {
        multiple: bool,
    },
    Value {
        layout: ValueLayout,
        connectable: bool,
    },
}

#[derive(Debug, Default)]
struct Schema {
    base: Option<String>,
    properties: BTreeMap<String, Property>,
}

/// Parsed Protein schemas with inherited properties resolved once per schema.
pub struct SchemaCatalog {
    schemas: HashMap<String, Schema>,
    properties: HashMap<String, BTreeMap<String, Property>>,
}

impl SchemaCatalog {
    /// Parse the schemas in one nested Protein archive through the decode session.
    /// Returns `None` when the archive has no schema entries.
    pub fn load<'a>(
        ctx: &DecodeContext<'a>,
        protein: View<'a>,
    ) -> Result<Option<Self>, CodecError> {
        let archive = ArchiveSnapshot::new(ctx, protein)?;
        let mut schemas = HashMap::new();
        for entry in archive.entries() {
            if !is_schema_entry(&entry.name) {
                continue;
            }
            if entry.uncompressed_size > MAX_SCHEMA_BYTES {
                return Err(CodecError::malformed(format_args!(
                    "Protein schema {} exceeds the {MAX_SCHEMA_BYTES}-byte limit",
                    entry.name
                )));
            }
            let xml = archive.open(ctx, &entry.name)?;
            parse_schema_document(Some(ctx), &entry.name, xml.window(), &mut schemas)?;
        }
        if schemas.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            schemas,
            properties: HashMap::new(),
        }))
    }

    /// Parse schemas for source-retaining export edits, which have no decode session.
    pub fn load_for_edit(protein: &[u8]) -> Result<Self, CodecError> {
        let schemas = schemas(protein)?;
        Ok(Self {
            schemas,
            properties: HashMap::new(),
        })
    }

    fn properties_for(
        &mut self,
        ctx: Option<&DecodeContext<'_>>,
        name: &str,
    ) -> Result<&BTreeMap<String, Property>, CodecError> {
        resolve_inheritance(ctx, &self.schemas, &mut self.properties, name)?;
        self.properties.get(name).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "Protein instance references absent schema {name}"
            ))
        })
    }
}

fn resolve_inheritance(
    ctx: Option<&DecodeContext<'_>>,
    schemas: &HashMap<String, Schema>,
    resolved: &mut HashMap<String, BTreeMap<String, Property>>,
    name: &str,
) -> Result<(), CodecError> {
    let mut path = Vec::new();
    let mut active = BTreeSet::new();
    let mut depth_guards = Vec::new();
    let mut current = name;
    while !resolved.contains_key(current) {
        if active.contains(current) {
            return Err(CodecError::malformed(format_args!(
                "Protein schema inheritance contains a cycle at {current}"
            )));
        }
        let schema = schemas.get(current).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "Protein instance references absent schema {current}"
            ))
        })?;
        if let Some(ctx) = ctx {
            depth_guards.push(ctx.enter_nested("Protein schema inheritance")?);
            ctx.charge_work(1, "Protein schema inheritance traversal")?;
            ctx.charge_collection_items(2, "Protein schema inheritance path")?;
        }
        active.insert(current);
        path.push(current);
        let Some(base) = schema.base.as_deref() else {
            break;
        };
        current = base;
    }
    while let Some(current) = path.pop() {
        let schema = schemas.get(current).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "Protein instance references absent schema {current}"
            ))
        })?;
        let inherited = schema.base.as_deref().and_then(|base| resolved.get(base));
        let inherited_count = inherited.map_or(0, BTreeMap::len);
        let copied_count = inherited_count
            .checked_add(schema.properties.len())
            .ok_or_else(|| CodecError::Malformed("Protein closure size overflows".into()))?;
        if let Some(ctx) = ctx {
            ctx.charge_work(copied_count as u64, "Protein inherited property closure")?;
            ctx.charge_collection_items(
                copied_count as u64 + 1,
                "Protein inherited property closure",
            )?;
            let copied_name_bytes = inherited
                .into_iter()
                .flat_map(|properties| properties.keys())
                .chain(schema.properties.keys())
                .try_fold(current.len() as u64, |total, id| {
                    total.checked_add(id.len() as u64)
                })
                .ok_or_else(|| {
                    CodecError::Malformed("Protein closure names length overflows".into())
                })?;
            ctx.charge_retained(copied_name_bytes, "Protein inherited property names")?;
        }
        let mut properties = inherited.cloned().unwrap_or_default();
        properties.extend(schema.properties.clone());
        resolved.insert(current.to_owned(), properties);
        drop(depth_guards.pop());
    }
    Ok(())
}

/// One paged Protein instance record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecodedRecord {
    /// Zero-based logical-record ordinal in the paged instance stream.
    pub ordinal: u64,
    /// Byte offset of the record in the dechunked logical stream.
    pub logical_offset: usize,
    /// Schema identifier selected by the record.
    pub schema: String,
    /// Asset instance GUID.
    pub guid: String,
    /// Base asset identifier.
    pub base: String,
    /// Library holding the preset this asset instantiates: a GUID for a shipped
    /// library, a path for a user library.
    pub asset_lib_id: String,
    /// Properties keyed by schema property identifier.
    pub properties: BTreeMap<String, DecodedProperty>,
}

/// One paged instance record rejected by schema-driven decoding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedRecord {
    /// Zero-based logical-record ordinal in the paged instance stream.
    pub ordinal: u64,
    /// Deterministic structural or schema error.
    pub detail: String,
}

/// Complete schema-driven result for one paged instance stream.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DecodeOutcome {
    /// Successfully decoded records in serialized order.
    pub records: Vec<DecodedRecord>,
    /// Records whose page boundary was valid but whose value block was not.
    pub rejected: Vec<RejectedRecord>,
}

/// Decode every instance record through one caller-owned decode session.
pub fn decode_detailed<'a>(
    ctx: &DecodeContext<'a>,
    protein: View<'a>,
    instance: View<'a>,
) -> Result<DecodeOutcome, CodecError> {
    let mut catalog = match SchemaCatalog::load(ctx, protein)? {
        Some(catalog) => catalog,
        None => SchemaCatalog {
            schemas: HashMap::new(),
            properties: HashMap::new(),
        },
    };
    let frames = framing::record_frames_admitted(ctx, instance.window())?;
    decode_frames_admitted(ctx, &mut catalog, &frames)
}

/// Decode already framed records for source-retaining export edits.
pub fn decode_frames_for_edit(
    protein: &[u8],
    frames: &[framing::RecordFrame],
) -> Result<DecodeOutcome, CodecError> {
    let mut catalog = SchemaCatalog::load_for_edit(protein)?;
    decode_frames(None, &mut catalog, frames)
}

/// Decode frames admitted by the caller against one parsed schema catalog.
pub fn decode_frames_admitted(
    ctx: &DecodeContext<'_>,
    catalog: &mut SchemaCatalog,
    frames: &[framing::RecordFrame],
) -> Result<DecodeOutcome, CodecError> {
    decode_frames(Some(ctx), catalog, frames)
}

fn decode_frames(
    ctx: Option<&DecodeContext<'_>>,
    catalog: &mut SchemaCatalog,
    frames: &[framing::RecordFrame],
) -> Result<DecodeOutcome, CodecError> {
    let mut outcome = DecodeOutcome::default();
    for (ordinal, frame) in frames.iter().enumerate() {
        let ordinal = u64::try_from(ordinal).map_err(|_| {
            CodecError::Malformed("Protein logical-record ordinal exceeds u64".into())
        })?;
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, "Protein record outcome")?;
        }
        match decode_record(ctx, frame.bytes(), catalog, ordinal, frame.logical_offset()) {
            Ok(Some(record)) => {
                outcome.records.push(record);
            }
            Ok(None) => {
                const DETAIL: &str = "Protein instance record header is malformed";
                if let Some(ctx) = ctx {
                    ctx.charge_retained(DETAIL.len() as u64, "Protein rejected record detail")?;
                }
                outcome.rejected.push(RejectedRecord {
                    ordinal,
                    detail: DETAIL.into(),
                });
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                if let Some(ctx) = ctx {
                    ctx.charge_retained(
                        rejection_detail_len(&error)?,
                        "Protein rejected record detail",
                    )?;
                }
                outcome.rejected.push(RejectedRecord {
                    ordinal,
                    detail: error.to_string(),
                });
            }
        }
    }
    Ok(outcome)
}

fn rejection_detail_len(error: &CodecError) -> Result<u64, CodecError> {
    struct Length(u64);
    impl std::fmt::Write for Length {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            self.0 = self
                .0
                .checked_add(text.len() as u64)
                .ok_or(std::fmt::Error)?;
            Ok(())
        }
    }
    let mut length = Length(0);
    std::fmt::write(&mut length, format_args!("{error}"))
        .map_err(|_| CodecError::Malformed("Protein rejection detail length overflows".into()))?;
    Ok(length.0)
}

/// Whether the Protein archive packages schema XML documents.
pub fn has_schemas(protein: &[u8]) -> bool {
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(protein)) else {
        return false;
    };
    (0..archive.len()).any(|index| {
        archive
            .by_index(index)
            .is_ok_and(|entry| is_schema_entry(entry.name()))
    })
}

fn is_schema_entry(name: &str) -> bool {
    (name.starts_with("Schemas/") || name.contains("/Schemas/")) && name.ends_with("Schema.xml")
}

fn schemas(protein: &[u8]) -> Result<HashMap<String, Schema>, CodecError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(protein)).map_err(|error| {
        CodecError::malformed(format_args!("cannot open nested Protein ZIP: {error}"))
    })?;
    let mut schemas = HashMap::new();
    let mut entry_names = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            CodecError::malformed(format_args!("cannot read nested Protein entry: {error}"))
        })?;
        if !entry_names.insert(entry.name().to_owned()) {
            return Err(CodecError::malformed(format_args!(
                "Protein archive defines entry {} more than once",
                entry.name()
            )));
        }
        if !is_schema_entry(entry.name()) {
            continue;
        }
        let size = entry.size();
        let name = entry.name().to_owned();
        let bytes = read_entry_bounded(&mut entry, size, &name)?;
        parse_schema_document(None, &name, &bytes, &mut schemas)?;
    }
    Ok(schemas)
}

fn parse_schema_document(
    ctx: Option<&DecodeContext<'_>>,
    name: &str,
    bytes: &[u8],
    schemas: &mut HashMap<String, Schema>,
) -> Result<(), CodecError> {
    let _reservation = if let Some(ctx) = ctx {
        let xml_len = u64::try_from(bytes.len())
            .map_err(|_| CodecError::Malformed("Protein schema XML length exceeds u64".into()))?;
        let mut tag_markers = 0_u64;
        let mut attribute_separators = 0_u64;
        for &byte in bytes {
            tag_markers += u64::from(byte == b'<');
            attribute_separators += u64::from(byte == b'=');
        }
        ctx.charge_work(xml_len, "Protein schema XML parse")?;
        ctx.charge_collection_items(tag_markers, "Protein schema XML nodes")?;
        // One tag can produce an element and adjacent text node. '=' bounds attributes.
        let possible_nodes = tag_markers
            .checked_mul(2)
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| {
                CodecError::Malformed("Protein schema XML node count overflows".into())
            })?;
        let materialized = xml_len
            .checked_mul(4)
            .and_then(|size| {
                possible_nodes
                    .checked_mul(XML_NODE_RESERVATION_BYTES)
                    .and_then(|nodes| size.checked_add(nodes))
            })
            .and_then(|size| {
                attribute_separators
                    .checked_mul(XML_ATTRIBUTE_RESERVATION_BYTES)
                    .and_then(|attributes| size.checked_add(attributes))
            })
            .ok_or_else(|| CodecError::Malformed("Protein schema XML size overflows".into()))?;
        Some(ctx.reserve_scoped(materialized, "Protein schema XML tree")?)
    } else {
        None
    };
    let xml = std::str::from_utf8(bytes).map_err(|error| {
        CodecError::malformed(format_args!("Protein schema {name} is not UTF-8: {error}"))
    })?;
    let document = roxmltree::Document::parse(xml).map_err(|error| {
        CodecError::malformed(format_args!(
            "Protein schema {name} is malformed XML: {error}"
        ))
    })?;
    let root = document.root_element();
    let uid = root
        .children()
        .find(|node| node.has_tag_name("UID"))
        .and_then(|node| node.attribute("val"))
        .ok_or_else(|| CodecError::malformed(format_args!("Protein schema {name} has no UID")))?;
    let mut schema = Schema::default();
    for node in root.children().filter(roxmltree::Node::is_element) {
        if node.has_tag_name("Base") {
            if let Some(value) = node.attribute("val") {
                if let Some(ctx) = ctx {
                    ctx.charge_retained(value.len() as u64, "Protein schema base name")?;
                }
                schema.base = Some(value.to_owned());
            }
            continue;
        }
        if node.has_tag_name("PropertyAlias") {
            continue;
        }
        if node.attribute("readonly") == Some("true")
            || node.attribute("definitionIteratorData") == Some("true")
        {
            continue;
        }
        let Some(property) = schema_property(node) else {
            continue;
        };
        let Some(id) = node.attribute("id") else {
            continue;
        };
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, "Protein schema property")?;
            ctx.charge_retained(id.len() as u64, "Protein schema property name")?;
        }
        schema.properties.insert(id.to_owned(), property);
    }
    if schemas.contains_key(uid) {
        return Err(CodecError::malformed(format_args!(
            "Protein archive defines schema {uid} more than once"
        )));
    }
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(1, "Protein parsed schema")?;
        ctx.charge_retained(uid.len() as u64, "Protein schema UID")?;
    }
    schemas.insert(uid.to_owned(), schema);
    Ok(())
}

fn schema_property(node: roxmltree::Node<'_, '_>) -> Option<Property> {
    let multiple = node.attribute("allowmultiplevalues") == Some("true");
    let connectable = node.attribute("allowconnectedassets").is_some();
    let carrier = match node.tag_name().name() {
        "Reference" => return Some(Property::Reference { multiple }),
        "TextureURI" => {
            return Some(Property::Value {
                layout: ValueLayout::TextureUri,
                connectable,
            });
        }
        "Boolean" => ValueCarrier::Boolean,
        "Integer" | "Choice" => ValueCarrier::Integer,
        "Float" if node.attribute("unit").is_some() => ValueCarrier::UnitFloat,
        "Float" => ValueCarrier::Float,
        "Distance" => ValueCarrier::Distance,
        "String" | "Uuid" | "URL" => ValueCarrier::String,
        "Color" => ValueCarrier::Color,
        _ => return None,
    };
    Some(Property::Value {
        layout: if multiple {
            ValueLayout::Multiple(carrier)
        } else {
            ValueLayout::Single(carrier)
        },
        connectable,
    })
}

fn decode_record(
    ctx: Option<&DecodeContext<'_>>,
    record: &[u8],
    catalog: &mut SchemaCatalog,
    ordinal: u64,
    logical_offset: usize,
) -> Result<Option<DecodedRecord>, CodecError> {
    if !record.starts_with(RECORD_MARKER) {
        return Ok(None);
    }
    let mut at = RECORD_MARKER.len();
    let Some(schema) = take_lp_utf8_capped(ctx, record, &mut at, 1_048_576)? else {
        return Ok(None);
    };
    let Some(guid) = take_lp_utf8_capped(ctx, record, &mut at, 1_048_576)? else {
        return Ok(None);
    };
    let Some(base) = take_lp_utf8_capped(ctx, record, &mut at, 1_048_576)? else {
        return Ok(None);
    };
    // The fourth header string is `AssetLibID`, the first member of
    // `CommonSchema` in serialization order. It is carried in the record header
    // rather than in the value block, so `instance_property_serializes` drops
    // the member there.
    let Some(asset_lib_id) = take_lp_utf8_capped(ctx, record, &mut at, 1_048_576)? else {
        return Ok(None);
    };
    let properties = catalog.properties_for(ctx, &schema)?;
    let mut values = BTreeMap::new();
    for (id, property) in properties {
        if !instance_property_serializes(id) {
            continue;
        }
        let property_at = at;
        let value_offset = if matches!(
            property,
            Property::Value {
                layout: ValueLayout::Single(ValueCarrier::UnitFloat | ValueCarrier::Distance),
                ..
            }
        ) {
            property_at.checked_add(4).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "Protein {schema} instance {guid} property {id} offset overflows usize"
                ))
            })?
        } else {
            property_at
        };
        let value_error = |error: CodecError, at: usize| {
            if matches!(&error, CodecError::ResourceLimit(_)) {
                return error;
            }
            CodecError::malformed(format_args!(
                "Protein {schema} instance {guid} property {id} at {property_at}..{at}/{}: {error}",
                record.len()
            ))
        };
        let connection_error = |error: CodecError, at: usize| {
            if matches!(&error, CodecError::ResourceLimit(_)) {
                return error;
            }
            CodecError::malformed(format_args!(
                "Protein {schema} instance {guid} property {id} connection at {at}/{}: {error}",
                record.len()
            ))
        };
        let content = match property {
            Property::Reference { multiple } => {
                let count = (*multiple)
                    .then(|| read_count(record, &mut at, id))
                    .transpose()
                    .map_err(|error| value_error(error, at))?;
                let targets = read_connections(ctx, record, &mut at)
                    .map_err(|error| connection_error(error, at))?;
                match count {
                    Some(count) => match std::num::NonZeroUsize::new(count) {
                        Some(count) => PropertyContent::MultipleReferences { count, targets },
                        None => PropertyContent::Value {
                            value: PropertyValue::Multiple(Vec::new()),
                            connections: targets,
                        },
                    },
                    None => PropertyContent::Reference(targets),
                }
            }
            Property::Value {
                layout,
                connectable,
            } => {
                let value = read_property(ctx, record, &mut at, *layout, id)
                    .map_err(|error| value_error(error, at))?;
                let connections = (*connectable)
                    .then(|| read_connections(ctx, record, &mut at))
                    .transpose()
                    .map_err(|error| connection_error(error, at))?
                    .unwrap_or_default();
                PropertyContent::Value { value, connections }
            }
        };
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, "Protein decoded property")?;
            ctx.charge_retained(id.len() as u64, "Protein decoded property name")?;
        }
        values.insert(
            id.clone(),
            DecodedProperty {
                value_offset,
                content,
            },
        );
    }
    if at != record.len() {
        return Err(CodecError::malformed(format_args!(
            "Protein {schema} instance {guid} consumed {at} of {} record bytes",
            record.len()
        )));
    }
    Ok(Some(DecodedRecord {
        ordinal,
        logical_offset,
        schema,
        guid,
        base,
        asset_lib_id,
        properties: values,
    }))
}

/// Narrow the inherited member set to the members a record actually serializes
/// (MA-08).
///
/// Two slots the closure lists do not appear in the value block.
/// `AssetLibID` is consumed as the fourth record header string. The second slot
/// belongs to `TextureMap2dSchema`: of `texture_MapChannel`,
/// `texture_MapChannel_ID_Advanced`, `texture_MapChannel_UVWSource_Advanced` and
/// `swatch`, exactly one is absent, and the two serialized integers hold `1` and
/// `0`. Dropping `texture_MapChannel_ID_Advanced` or `texture_MapChannel` leaves
/// every remaining member at its schema default; dropping either of the other two
/// forces a member away from its default, so both are excluded. Which of the
/// surviving pair the writer omits is not decidable from the bytes.
fn instance_property_serializes(id: &str) -> bool {
    !matches!(id, "AssetLibID" | "texture_MapChannel_ID_Advanced")
}

fn read_property(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    at: &mut usize,
    layout: ValueLayout,
    id: &str,
) -> Result<PropertyValue, CodecError> {
    match layout {
        ValueLayout::Single(carrier) => read_value(ctx, bytes, at, carrier, id),
        // TextureURI owns its kind byte and optional count; the schema's
        // multiple-value declaration does not add another count prefix.
        ValueLayout::TextureUri => read_texture_uri(ctx, bytes, at, id),
        ValueLayout::Multiple(carrier) => {
            let count = read_count(bytes, at, id)?;
            if let Some(ctx) = ctx {
                ctx.charge_collection_items(count as u64, "Protein multiple property members")?;
            }
            let mut values = Vec::with_capacity(count);
            for _ in 0..count {
                values.push(read_value(ctx, bytes, at, carrier, id)?);
            }
            Ok(PropertyValue::Multiple(values))
        }
    }
}

/// A `TextureURI` value: a kind byte, then either a counted list of paths
/// (kind 0, used for cloud resource references) or a single path (kind 1).
fn read_texture_uri(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    at: &mut usize,
    id: &str,
) -> Result<PropertyValue, CodecError> {
    let malformed = || CodecError::malformed(format_args!("Protein property {id} is truncated"));
    let kind = take::<1>(bytes, at).ok_or_else(malformed)?[0];
    if kind == 1 {
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, "Protein texture URI paths")?;
        }
        return Ok(PropertyValue::TextureUri(vec![take_lp_utf8_capped(
            ctx, bytes, at, 1_048_576,
        )?
        .ok_or_else(malformed)?]));
    }
    if kind != 0 {
        return Err(CodecError::malformed(format_args!(
            "Protein TextureURI property {id} has invalid kind {kind}"
        )));
    }
    let count = read_count(bytes, at, id)?;
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(count as u64, "Protein texture URI paths")?;
    }
    let mut paths = Vec::with_capacity(count);
    for _ in 0..count {
        paths.push(take_lp_utf8_capped(ctx, bytes, at, 1_048_576)?.ok_or_else(malformed)?);
    }
    Ok(PropertyValue::TextureUri(paths))
}

fn read_count(bytes: &[u8], at: &mut usize, id: &str) -> Result<usize, CodecError> {
    let count = usize::try_from(read_u32_le(bytes, at).ok_or_else(|| {
        CodecError::malformed(format_args!("Protein property {id} is truncated"))
    })?)
    .map_err(|_| CodecError::Malformed("Protein value count exceeds usize".into()))?;
    if count > 1_024 {
        return Err(CodecError::malformed(format_args!(
            "Protein property {id} has implausible value count {count}"
        )));
    }
    Ok(count)
}

fn read_u32_le(bytes: &[u8], at: &mut usize) -> Option<u32> {
    let value = View::u32_le_at(bytes, *at)?;
    *at = (*at).checked_add(4)?;
    Some(value)
}

fn read_f64_le(bytes: &[u8], at: &mut usize) -> Option<f64> {
    let value = View::f64_le_at(bytes, *at)?;
    *at = (*at).checked_add(8)?;
    Some(value)
}

fn read_value(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    at: &mut usize,
    carrier: ValueCarrier,
    id: &str,
) -> Result<PropertyValue, CodecError> {
    let malformed = || CodecError::malformed(format_args!("Protein property {id} is truncated"));
    Ok(match carrier {
        ValueCarrier::Boolean => {
            PropertyValue::Boolean(take::<1>(bytes, at).ok_or_else(malformed)?[0] != 0)
        }
        ValueCarrier::Integer => {
            PropertyValue::Integer(read_u32_le(bytes, at).ok_or_else(malformed)?)
        }
        ValueCarrier::Float => PropertyValue::Float(finite_value(
            read_f64_le(bytes, at).ok_or_else(malformed)?,
            id,
        )?),
        ValueCarrier::UnitFloat => {
            take::<4>(bytes, at).ok_or_else(malformed)?;
            PropertyValue::Float(finite_value(
                read_f64_le(bytes, at).ok_or_else(malformed)?,
                id,
            )?)
        }
        ValueCarrier::Distance => PropertyValue::Distance {
            unit: read_u32_le(bytes, at).ok_or_else(malformed)?,
            value: finite_value(read_f64_le(bytes, at).ok_or_else(malformed)?, id)?,
        },
        ValueCarrier::String => PropertyValue::String(
            take_lp_utf8_capped(ctx, bytes, at, 1_048_576)?.ok_or_else(malformed)?,
        ),
        ValueCarrier::Color => {
            let mut rgba = [0.0; 4];
            for value in &mut rgba {
                *value = finite_value(read_f64_le(bytes, at).ok_or_else(malformed)?, id)?;
            }
            PropertyValue::Color(rgba)
        }
    })
}

fn finite_value(value: f64, id: &str) -> Result<f64, CodecError> {
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| CodecError::malformed(format_args!("Protein property {id} is not finite")))
}

/// The connection block that follows every connectable member and every
/// `Reference`: a presence byte, then a kind byte, a `u32` count, and that many
/// length-prefixed connected-asset GUIDs.
fn read_connections(
    ctx: Option<&DecodeContext<'_>>,
    bytes: &[u8],
    at: &mut usize,
) -> Result<Vec<String>, CodecError> {
    let Some(present) = take::<1>(bytes, at) else {
        return Err(CodecError::Malformed(
            "Protein property connection flag is truncated".into(),
        ));
    };
    if present == [0] {
        return Ok(Vec::new());
    }
    if present != [1] {
        return Err(CodecError::malformed(format_args!(
            "Protein property has invalid connection flag {}",
            present[0]
        )));
    }
    let kind = take::<1>(bytes, at).ok_or_else(|| {
        CodecError::Malformed("Protein property connection kind is truncated".into())
    })?;
    if kind != [1] {
        return Err(CodecError::malformed(format_args!(
            "Protein property has invalid connection kind {}",
            kind[0]
        )));
    }
    let count = read_count(bytes, at, "connection")?;
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(count as u64, "Protein connected asset GUIDs")?;
    }
    let mut connections = Vec::with_capacity(count);
    for _ in 0..count {
        connections.push(
            take_lp_utf8_capped(ctx, bytes, at, 1_048_576)?.ok_or_else(|| {
                CodecError::Malformed("Protein property connection GUID is truncated".into())
            })?,
        );
    }
    Ok(connections)
}

fn take<const N: usize>(bytes: &[u8], at: &mut usize) -> Option<[u8; N]> {
    let end = at.checked_add(N)?;
    let value = bytes.get(*at..end)?.try_into().ok()?;
    *at = end;
    Some(value)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)] // A failed synthetic decode is the test failure.

    use std::io::{Cursor, Write};

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    use super::{
        framing, instance_property_serializes, read_connections, read_texture_uri, read_value,
        ValueCarrier, CONTINUATION_MARKER, PAGE_SIZE, RECORD_MARKER, STREAM_HEADER_LEN,
        TERMINAL_MARKER,
    };
    use crate::property::{PropertyContent, PropertyValue};

    fn decode_fixture(protein: &[u8], instance: &[u8]) -> Result<super::DecodeOutcome, CodecError> {
        let mut bytes = protein.to_vec();
        bytes.extend_from_slice(instance);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())?;
        let protein_view = root.child(0, protein.len()).expect("fixture Protein range");
        let instance_view = root
            .child(protein.len(), bytes.len())
            .expect("fixture instance range");
        super::decode_detailed(&ctx, protein_view, instance_view)
    }

    #[test]
    fn schema_expansion_refuses_before_xml_materialization() {
        let xml = br#"<Schema><UID val="Simple"/><String id="comment"/></Schema>"#;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "Schemas/SimpleSchema.xml",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .expect("schema entry starts");
        writer.write_all(xml).expect("schema XML writes");
        let protein = writer.finish().expect("archive finishes").into_inner();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_decompressed_bytes_per_expand = xml.len() as u64 - 1;
        let (ctx, root) = DecodeContext::from_root_bytes(&protein, &arena, &policy)
            .expect("ZIP fits input limit");
        assert!(matches!(
            super::SchemaCatalog::load(&ctx, root),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::DecompressedBytes
        ));
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&protein, &arena, &DecodePolicy::service())
                .expect("ZIP fits service input limit");
        assert!(super::SchemaCatalog::load(&ctx, root)
            .expect("schema fits service profile")
            .is_some());
    }

    #[test]
    fn schema_inheritance_refuses_at_active_depth_limit() {
        let schemas = std::collections::HashMap::from([
            (
                "AChild".into(),
                super::Schema {
                    base: Some("ZBase".into()),
                    ..super::Schema::default()
                },
            ),
            ("ZBase".into(), super::Schema::default()),
        ]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fixture fits input limit");
        assert!(matches!(
            super::resolve_inheritance(Some(&ctx), &schemas, &mut std::collections::HashMap::new(), "AChild"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RecursionDepth
        ));
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("fixture fits service profile");
        assert!(super::resolve_inheritance(
            Some(&ctx),
            &schemas,
            &mut std::collections::HashMap::new(),
            "AChild"
        )
        .is_ok());
    }

    #[test]
    fn unused_schema_with_absent_base_does_not_reject_selected_record() {
        let mut record = Vec::new();
        for value in ["Good", "guid", "base", ""] {
            push_lp(&mut record, value);
        }
        let stream = paged_stream(&[&record]);
        let frames = framing::record_frames_for_edit(&stream).expect("fixture framing is valid");
        let mut catalog = super::SchemaCatalog {
            schemas: std::collections::HashMap::from([
                ("Good".into(), super::Schema::default()),
                (
                    "Unused".into(),
                    super::Schema {
                        base: Some("Absent".into()),
                        ..super::Schema::default()
                    },
                ),
            ]),
            properties: std::collections::HashMap::new(),
        };
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &DecodePolicy::service())
            .expect("stream fits service input limit");
        let outcome = super::decode_frames_admitted(&ctx, &mut catalog, &frames)
            .expect("unused schema is not resolved");
        assert_eq!(outcome.records.len(), 1);
        assert!(outcome.rejected.is_empty());
    }

    #[test]
    fn schema_xml_tree_refuses_before_parse_allocation() {
        let xml = br#"<Schema><UID val="Simple"/><String id="comment"/></Schema>"#;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = xml.len() as u64 * 4 - 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(xml, &arena, &policy).expect("XML fits input limit");
        assert!(matches!(
            super::parse_schema_document(
                Some(&ctx),
                "Schemas/SimpleSchema.xml",
                xml,
                &mut std::collections::HashMap::new(),
            ),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::MaterializedBytes
                    && limit.operation == "Protein schema XML tree"
        ));
    }

    #[test]
    fn dense_schema_xml_reserves_node_and_attribute_storage() {
        let xml = br#"<Schema><UID val="Simple"/><String id="a"/><String id="b"/><String id="c"/></Schema>"#;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = xml.len() as u64 * 4;
        let (ctx, _) =
            DecodeContext::from_root_bytes(xml, &arena, &policy).expect("XML fits input limit");
        assert!(matches!(
            super::parse_schema_document(
                Some(&ctx),
                "Schemas/SimpleSchema.xml",
                xml,
                &mut std::collections::HashMap::new(),
            ),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::MaterializedBytes
                    && limit.operation == "Protein schema XML tree"
        ));
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(xml, &arena, &DecodePolicy::service())
            .expect("XML fits service profile");
        assert!(super::parse_schema_document(
            Some(&ctx),
            "Schemas/SimpleSchema.xml",
            xml,
            &mut std::collections::HashMap::new(),
        )
        .is_ok());
    }

    #[test]
    fn parsed_schema_refuses_before_catalog_insertion() {
        let xml = br#"<Schema><UID val="Simple"/><String id="comment"/></Schema>"#;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 5;
        let (ctx, _) =
            DecodeContext::from_root_bytes(xml, &arena, &policy).expect("XML fits input limit");
        assert!(matches!(
            super::parse_schema_document(
                Some(&ctx),
                "Schemas/SimpleSchema.xml",
                xml,
                &mut std::collections::HashMap::new(),
            ),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "Protein parsed schema"
        ));
    }

    #[test]
    fn inheritance_work_refuses_before_property_closure_copy() {
        let schemas = std::collections::HashMap::from([(
            "Simple".into(),
            super::Schema {
                properties: std::collections::BTreeMap::from([
                    (
                        "first".into(),
                        super::Property::Reference { multiple: false },
                    ),
                    (
                        "second".into(),
                        super::Property::Reference { multiple: false },
                    ),
                ]),
                ..super::Schema::default()
            },
        )]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 2;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fixture fits input limit");
        assert!(matches!(
            super::resolve_inheritance(Some(&ctx), &schemas, &mut std::collections::HashMap::new(), "Simple"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
                    && limit.operation == "Protein inherited property closure"
        ));
    }

    #[test]
    fn logical_frame_count_refuses_before_record_copy() {
        let stream = paged_stream(&[b"frame"]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("stream fits input limit");
        assert!(matches!(
            framing::record_frames_admitted(&ctx, &stream),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "Protein logical record frame"
        ));
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &DecodePolicy::service())
            .expect("stream fits service profile");
        assert_eq!(
            framing::record_frames_admitted(&ctx, &stream)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn admitted_frames_decode_once_with_one_outcome_charge() {
        let mut record = Vec::new();
        for value in ["Simple", "guid", "base", ""] {
            push_lp(&mut record, value);
        }
        let stream = paged_stream(&[&record]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("stream fits input limit");
        let frames = framing::record_frames_admitted(&ctx, &stream)
            .expect("one frame fits the collection limit");
        let mut catalog = super::SchemaCatalog {
            schemas: std::collections::HashMap::new(),
            properties: std::collections::HashMap::from([(
                "Simple".into(),
                std::collections::BTreeMap::new(),
            )]),
        };
        let outcome = super::decode_frames_admitted(&ctx, &mut catalog, &frames)
            .expect("one outcome needs no second framing pass");
        assert_eq!(outcome.records.len(), 1);
        assert!(matches!(
            ctx.charge_collection_items(1, "probe after frame and outcome"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
        ));
    }

    #[test]
    fn multiple_property_members_refuse_before_vector_allocation() {
        let mut bytes = 2_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("property fits input limit");
        assert!(matches!(
            super::read_property(
                Some(&ctx),
                &bytes,
                &mut 0,
                super::ValueLayout::Multiple(ValueCarrier::Integer),
                "values",
            ),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "Protein multiple property members"
        ));
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("property fits service profile");
        assert!(super::read_property(
            Some(&ctx),
            &bytes,
            &mut 0,
            super::ValueLayout::Multiple(ValueCarrier::Integer),
            "values",
        )
        .is_ok());
    }

    #[test]
    fn texture_uri_paths_refuse_before_vector_allocation() {
        let mut bytes = vec![0];
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        push_lp(&mut bytes, "first");
        push_lp(&mut bytes, "second");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("paths fit input limit");
        assert!(matches!(
            read_texture_uri(Some(&ctx), &bytes, &mut 0, "bitmap"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "Protein texture URI paths"
        ));
    }

    #[test]
    fn connected_asset_guids_refuse_before_vector_allocation() {
        let mut bytes = vec![1, 1];
        bytes.extend_from_slice(&2_u32.to_le_bytes());
        push_lp(&mut bytes, "first");
        push_lp(&mut bytes, "second");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("connections fit input limit");
        assert!(matches!(
            read_connections(Some(&ctx), &bytes, &mut 0),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "Protein connected asset GUIDs"
        ));
    }

    #[test]
    fn copied_record_range_refuses_before_frame_growth() {
        let stream = paged_stream(&[b"frame"]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("stream fits input limit");
        assert!(matches!(
            framing::record_frames_admitted(&ctx, &stream),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "Protein copied record range"
        ));
    }

    #[test]
    fn decoded_outcome_refuses_before_rejection_vector_growth() {
        let stream = paged_stream(&[b"bad header"]);
        let frames = framing::record_frames_for_edit(&stream).expect("fixture framing is valid");
        let mut catalog = super::SchemaCatalog {
            schemas: std::collections::HashMap::new(),
            properties: std::collections::HashMap::new(),
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("stream fits input limit");
        assert!(matches!(
            super::decode_frames_admitted(&ctx, &mut catalog, &frames),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "Protein record outcome"
        ));
    }

    #[test]
    fn rejected_record_detail_refuses_before_string_allocation() {
        let mut record = Vec::new();
        for value in ["Absent", "guid", "base", ""] {
            push_lp(&mut record, value);
        }
        let stream = paged_stream(&[&record]);
        let frames = framing::record_frames_for_edit(&stream).expect("fixture framing is valid");
        let mut catalog = super::SchemaCatalog {
            schemas: std::collections::HashMap::new(),
            properties: std::collections::HashMap::new(),
        };
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 14;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("stream fits input limit");
        assert!(matches!(
            super::decode_frames_admitted(&ctx, &mut catalog, &frames),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "Protein rejected record detail"
        ));
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &DecodePolicy::service())
            .expect("stream fits service profile");
        assert_eq!(
            super::decode_frames_admitted(&ctx, &mut catalog, &frames)
                .expect("rejection detail fits service profile")
                .rejected
                .len(),
            1
        );
    }

    #[test]
    fn nested_property_resource_refusal_is_not_a_rejected_record() {
        let mut catalog = super::SchemaCatalog {
            schemas: std::collections::HashMap::new(),
            properties: std::collections::HashMap::from([(
                "Simple".into(),
                std::collections::BTreeMap::from([(
                    "values".into(),
                    super::Property::Value {
                        layout: super::ValueLayout::Multiple(ValueCarrier::Integer),
                        connectable: false,
                    },
                )]),
            )]),
        };
        let mut record = Vec::new();
        for value in ["Simple", "guid", "base", ""] {
            push_lp(&mut record, value);
        }
        record.extend_from_slice(&2_u32.to_le_bytes());
        record.extend_from_slice(&1_u32.to_le_bytes());
        record.extend_from_slice(&2_u32.to_le_bytes());
        let stream = paged_stream(&[&record]);
        let frames = framing::record_frames_for_edit(&stream).expect("fixture framing is valid");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("stream fits input limit");
        assert!(matches!(
            super::decode_frames_admitted(&ctx, &mut catalog, &frames),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "Protein multiple property members"
        ));
    }

    #[test]
    fn inherited_property_selection_drops_the_header_slot_and_texture_swatch() {
        assert!(!instance_property_serializes("AssetLibID"));
        assert!(!instance_property_serializes(
            "texture_MapChannel_ID_Advanced"
        ));
        assert!(instance_property_serializes("ExchangeGUID"));
        assert!(instance_property_serializes("swatch"));
        assert!(instance_property_serializes("interior_model"));
        assert!(instance_property_serializes("texture_MapChannel"));
        assert!(instance_property_serializes(
            "texture_MapChannel_UVWSource_Advanced"
        ));
        assert!(instance_property_serializes("common_Shared_Asset"));
        assert!(instance_property_serializes("common_Tint_color_colorspace"));
    }

    #[test]
    fn color_carries_no_marker_byte_whether_or_not_it_is_connectable() {
        let rgba = [0.1_f64, 0.2, 0.3, 1.0];
        let bare = rgba
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        for id in ["metal_f0", "common_Tint_color"] {
            let mut at = 0;
            assert_eq!(
                read_value(None, &bare, &mut at, ValueCarrier::Color, id).unwrap(),
                PropertyValue::Color(rgba)
            );
            assert_eq!(at, bare.len());
        }
    }

    #[test]
    fn connection_and_texture_uri_blocks_carry_a_kind_byte() {
        let mut connections = vec![1, 1];
        connections.extend_from_slice(&2u32.to_le_bytes());
        push_lp(&mut connections, "first-guid");
        push_lp(&mut connections, "second-guid");
        let mut at = 0;
        assert_eq!(
            read_connections(None, &connections, &mut at).unwrap(),
            ["first-guid", "second-guid"]
        );
        assert_eq!(at, connections.len());

        let mut at = 0;
        assert!(read_connections(None, &[0], &mut at).unwrap().is_empty());
        assert_eq!(at, 1);

        let mut counted = vec![0];
        counted.extend_from_slice(&1u32.to_le_bytes());
        push_lp(&mut counted, "cloud/resource/one");
        let mut at = 0;
        assert_eq!(
            read_texture_uri(None, &counted, &mut at, "unifiedbitmap_Bitmap").unwrap(),
            PropertyValue::TextureUri(vec!["cloud/resource/one".into()])
        );
        assert_eq!(at, counted.len());

        let mut single = vec![1];
        push_lp(&mut single, "local_bitmap.png");
        let mut at = 0;
        assert_eq!(
            read_texture_uri(None, &single, &mut at, "unifiedbitmap_Bitmap").unwrap(),
            PropertyValue::TextureUri(vec!["local_bitmap.png".into()])
        );
        assert_eq!(at, single.len());
    }

    #[test]
    fn multiple_references_keep_one_connection_block_including_zero_values() {
        let protein = schema_archive(&[(
            "Schemas/ReferencesSchema.xml",
            r#"<Schema><UID val="References"/><Reference id="targets" allowmultiplevalues="true"/></Schema>"#,
        )]);
        for count in [0_u32, 2] {
            let mut record = Vec::new();
            for value in ["References", "asset-guid", "Reference", ""] {
                push_lp(&mut record, value);
            }
            record.extend_from_slice(&count.to_le_bytes());
            push_connections(&mut record, &["target"]);
            let outcome = decode_fixture(&protein, &paged_stream(&[&record])).unwrap();
            assert!(outcome.rejected.is_empty(), "{:?}", outcome.rejected);
            let records = outcome.records;
            assert_eq!(records.len(), 1);
            assert_eq!(
                records[0].properties["targets"].value().is_none(),
                count != 0
            );
            assert_eq!(
                records[0].properties["targets"].content,
                match std::num::NonZeroUsize::new(count as usize) {
                    Some(count) => PropertyContent::MultipleReferences {
                        count,
                        targets: vec!["target".into()],
                    },
                    None => PropertyContent::Value {
                        value: PropertyValue::Multiple(Vec::new()),
                        connections: vec!["target".into()],
                    },
                }
            );
        }
    }

    #[test]
    fn schema_driven_record_uses_inheritance_and_serialized_property_ids() {
        let protein = schema_archive(&[
            (
                "Schemas/CommonSchema.xml",
                r#"<Schema>
                    <UID val="CommonSchema"/>
                    <String id="AssetLibID" val=""/>
                    <Uuid id="ExchangeGUID" val=""/>
                    <Color id="a_color" allowconnectedassets="single"/>
                    <Boolean id="ignored_readonly" readonly="true"/>
                    <Integer id="revision" public="false" val="1"/>
                </Schema>"#,
            ),
            (
                "Asset/Schemas/TextureSchema.xml",
                r#"<Schema>
                    <UID val="TextureSchema"/>
                    <Base val="CommonSchema"/>
                    <PropertyAlias id="renamed_color" property="a_color"/>
                    <Distance id="b_distance"/>
                    <TextureURI id="c_uri" allowmultiplevalues="true"/>
                    <Float id="d_unit_float" unit="unitless"/>
                    <Reference id="e_reference"/>
                    <Float id="f_profile" allowmultiplevalues="true"/>
                    <String id="swatch" public="false" val=""/>
                    <Integer id="ignored_definition" definitionIteratorData="true"/>
                    <String id="metadata_still_serializes" metadata="true"/>
                </Schema>"#,
            ),
        ]);
        let mut values = Vec::new();
        push_lp(&mut values, ""); // ExchangeGUID
        for value in [0.1_f64, 0.2, 0.3, 1.0] {
            values.extend_from_slice(&value.to_le_bytes()); // a_color, no marker
        }
        push_connections(&mut values, &["first-guid", "second-guid"]);
        values.extend_from_slice(&0x2016_u32.to_le_bytes());
        values.extend_from_slice(&2.5_f64.to_le_bytes()); // b_distance
        values.push(0);
        values.extend_from_slice(&2u32.to_le_bytes());
        push_lp(&mut values, "cloud/resource/one");
        push_lp(&mut values, "cloud/resource/two"); // c_uri
        values.extend_from_slice(&0x200e_u32.to_le_bytes());
        values.extend_from_slice(&4.5_f64.to_le_bytes()); // d_unit_float
        push_connections(&mut values, &["reference-guid"]); // e_reference
        values.extend_from_slice(&2u32.to_le_bytes());
        values.extend_from_slice(&0.25_f64.to_le_bytes());
        values.extend_from_slice(&0.75_f64.to_le_bytes()); // f_profile
        push_lp(&mut values, "Comments"); // metadata_still_serializes
        values.extend_from_slice(&1u32.to_le_bytes()); // revision
        push_lp(&mut values, "Swatch-Torus"); // swatch

        let mut record = Vec::new();
        for value in ["TextureSchema", "asset-guid", "Texture", ""] {
            push_lp(&mut record, value);
        }
        record.extend_from_slice(&values);

        let outcome =
            decode_fixture(&protein, &paged_stream(&[&record])).expect("schema record decodes");
        assert!(outcome.rejected.is_empty());
        let records = outcome.records;
        assert_eq!(records.len(), 1);
        let properties = &records[0].properties;
        assert_eq!(
            properties["a_color"].value().unwrap().clone(),
            PropertyValue::Color([0.1, 0.2, 0.3, 1.0])
        );
        assert_eq!(
            properties["a_color"].connections(),
            ["first-guid", "second-guid"]
        );
        assert!(properties["a_color"].value_offset > RECORD_MARKER.len());
        assert_eq!(
            properties["b_distance"].value().unwrap().clone(),
            PropertyValue::Distance {
                unit: 0x2016,
                value: 2.5,
            }
        );
        assert_eq!(
            properties["c_uri"].value().unwrap().clone(),
            PropertyValue::TextureUri(vec![
                "cloud/resource/one".into(),
                "cloud/resource/two".into(),
            ])
        );
        assert_eq!(
            properties["d_unit_float"].value().unwrap().clone(),
            PropertyValue::Float(4.5)
        );
        assert_eq!(
            properties["e_reference"].connections(),
            vec!["reference-guid"]
        );
        assert_eq!(
            properties["f_profile"].value().unwrap().clone(),
            PropertyValue::Multiple(vec![PropertyValue::Float(0.25), PropertyValue::Float(0.75)])
        );
        assert_eq!(
            properties["metadata_still_serializes"]
                .value()
                .unwrap()
                .clone(),
            PropertyValue::String("Comments".into())
        );
        // `public="false"` does not suppress serialization.
        assert_eq!(
            properties["revision"].value().unwrap().clone(),
            PropertyValue::Integer(1)
        );
        assert_eq!(
            properties["swatch"].value().unwrap().clone(),
            PropertyValue::String("Swatch-Torus".into())
        );
        assert_eq!(
            properties["ExchangeGUID"].value().unwrap().clone(),
            PropertyValue::String(String::new())
        );
        // Consumed as the fourth record header string.
        assert!(!properties.contains_key("AssetLibID"));
        assert!(!properties.contains_key("renamed_color"));
        assert!(!properties.contains_key("ignored_readonly"));
        assert!(!properties.contains_key("ignored_definition"));
    }

    #[test]
    fn detailed_decode_accounts_for_a_rejected_record_and_continues() {
        let protein = schema_archive(&[(
            "Schemas/SimpleSchema.xml",
            r#"<Schema><UID val="SimpleSchema"/><String id="comment"/></Schema>"#,
        )]);
        let record = |guid: &str| {
            let mut bytes = Vec::new();
            for value in ["SimpleSchema", guid, "Simple", ""] {
                push_lp(&mut bytes, value);
            }
            push_lp(&mut bytes, &"x".repeat(160));
            bytes
        };
        let first = record("first-guid");
        let malformed = vec![0xff; 160];
        let third = record("third-guid");
        let instance = paged_stream(&[&first, &malformed, &third]);

        let outcome = decode_fixture(&protein, &instance).expect("framed records decode");
        assert_eq!(
            outcome
                .records
                .iter()
                .map(|record| (record.ordinal, record.guid.as_str()))
                .collect::<Vec<_>>(),
            [(0, "first-guid"), (2, "third-guid")]
        );
        assert_eq!(outcome.rejected.len(), 1);
        assert_eq!(outcome.rejected[0].ordinal, 1);
        assert!(!outcome.rejected[0].detail.is_empty());
    }

    #[test]
    fn detailed_decode_rejects_invalid_page_framing() {
        let protein = schema_archive(&[(
            "Schemas/SimpleSchema.xml",
            r#"<Schema><UID val="SimpleSchema"/><String id="comment"/></Schema>"#,
        )]);
        let error = decode_fixture(&protein, &[0; 16])
            .expect_err("a header without any complete page is malformed");
        assert!(matches!(error, CodecError::Malformed(message)
            if message == "Protein page stream is shorter than its header and one page"));
    }

    #[test]
    fn texture_records_omit_the_advanced_map_channel_id() {
        let protein = schema_archive(&[(
            "Schemas/BitmapSchema.xml",
            r#"<Schema>
                <UID val="BitmapSchema"/>
                <String id="AssetLibID" val=""/>
                <Integer id="texture_MapChannel" val="1"/>
                <Integer id="texture_MapChannel_ID_Advanced" val="1"/>
                <Integer id="texture_MapChannel_UVWSource_Advanced" val="0"/>
                <String id="swatch" public="false" val=""/>
            </Schema>"#,
        )]);
        let mut record = Vec::new();
        let padded_name = "Bitmap".repeat(32);
        for value in ["BitmapSchema", "asset-guid", &padded_name, ""] {
            push_lp(&mut record, value);
        }
        push_lp(&mut record, ""); // swatch
        record.extend_from_slice(&1u32.to_le_bytes()); // texture_MapChannel
        record.extend_from_slice(&0u32.to_le_bytes()); // ..._UVWSource_Advanced
        let outcome =
            decode_fixture(&protein, &paged_stream(&[&record])).expect("texture record decodes");
        assert!(outcome.rejected.is_empty());
        let records = outcome.records;
        assert_eq!(records.len(), 1);
        let properties = &records[0].properties;
        assert!(!properties.contains_key("texture_MapChannel_ID_Advanced"));
        assert_eq!(
            properties["texture_MapChannel"].value().unwrap().clone(),
            PropertyValue::Integer(1)
        );
        assert_eq!(
            properties["texture_MapChannel_UVWSource_Advanced"]
                .value()
                .unwrap()
                .clone(),
            PropertyValue::Integer(0)
        );
        assert_eq!(
            properties["swatch"].value().unwrap().clone(),
            PropertyValue::String(String::new())
        );
    }

    #[test]
    fn a_record_spanning_several_pages_ends_at_its_terminal_page() {
        let long = "x".repeat(400);
        let mut first = Vec::new();
        for value in ["S", "guid-one", &long, ""] {
            push_lp(&mut first, value);
        }
        let short = "y".repeat(140);
        let mut second = Vec::new();
        for value in ["S", "guid-two", &short, ""] {
            push_lp(&mut second, value);
        }
        let stream = paged_stream(&[&first, &second]);
        let frames = framing::record_frames_for_edit(&stream).expect("stream is paged");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].logical_offset(), 0);
        assert_eq!(frames[0].bytes(), [RECORD_MARKER, &first].concat());
        assert_eq!(frames[1].logical_offset(), frames[0].bytes().len());
        assert_eq!(frames[1].bytes(), [RECORD_MARKER, &second].concat());
        assert!(stream.len() > 16 + 3 * PAGE_SIZE, "record one spans pages");

        assert!(framing::record_frames_for_edit(&[]).is_err());
        let mut truncated = stream.clone();
        truncated.truncate(16 + PAGE_SIZE + 1);
        assert!(framing::record_frames_for_edit(&truncated).is_err());
    }

    #[test]
    fn standalone_terminal_page_carries_one_short_record() {
        let mut record = Vec::new();
        for value in ["S", "guid", "base", "library"] {
            push_lp(&mut record, value);
        }
        let mut stream = (PAGE_SIZE as u32).to_le_bytes().to_vec();
        stream.resize(STREAM_HEADER_LEN, 0);
        stream.extend_from_slice(TERMINAL_MARKER);
        stream.extend_from_slice(&(record.len() as u16).to_le_bytes());
        stream.extend_from_slice(&[1, 0]);
        stream.extend_from_slice(&record);
        stream.resize(STREAM_HEADER_LEN + PAGE_SIZE, 0);

        let frames = framing::record_frames_for_edit(&stream).expect("standalone terminal page");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].logical_offset(), 0);
        assert_eq!(frames[0].bytes(), [RECORD_MARKER, &record].concat());
    }

    /// Lay records out as `InstanceProperties.bin` does: a 16-byte stream header,
    /// then a marker page, continuation pages, and a terminal page per record.
    fn paged_stream(records: &[&[u8]]) -> Vec<u8> {
        const BODY: usize = PAGE_SIZE - 8;
        let mut out = (PAGE_SIZE as u32).to_le_bytes().to_vec();
        out.resize(STREAM_HEADER_LEN, 0);
        let mut page = |header: [u8; 8], body: &[u8]| {
            out.extend_from_slice(&header);
            out.extend_from_slice(body);
            out.resize(out.len() + BODY - body.len(), 0);
        };
        let opening = |marker: &[u8]| {
            let mut header = [0_u8; 8];
            header[4..8].copy_from_slice(marker);
            header
        };
        for record in records {
            // A marker or continuation page always contributes its whole body,
            // so only the terminal page can hold a partial tail.
            if record.len() < BODY {
                let mut header = [0_u8; 8];
                header[..4].copy_from_slice(TERMINAL_MARKER);
                header[4..6].copy_from_slice(&(record.len() as u16).to_le_bytes());
                page(header, record);
                continue;
            }
            let (head, rest) = record.split_at(BODY);
            page(opening(RECORD_MARKER), head);
            let mut chunks = rest.chunks(BODY).peekable();
            while let Some(chunk) = chunks.next() {
                if chunks.peek().is_some() {
                    page(opening(CONTINUATION_MARKER), chunk);
                } else {
                    let mut header = [0_u8; 8];
                    header[0..4].copy_from_slice(TERMINAL_MARKER);
                    header[4..6].copy_from_slice(&(chunk.len() as u16).to_le_bytes());
                    page(header, chunk);
                }
            }
        }
        out
    }

    fn schema_archive(entries: &[(&str, &str)]) -> Vec<u8> {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .system(zip::System::Unix);
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, xml) in entries {
            archive.start_file(name, options).expect("start schema");
            archive.write_all(xml.as_bytes()).expect("write schema");
        }
        archive.finish().expect("finish schemas").into_inner()
    }

    fn push_lp(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    fn push_connections(bytes: &mut Vec<u8>, values: &[&str]) {
        bytes.extend_from_slice(&[1, 1]);
        bytes.extend_from_slice(&(values.len() as u32).to_le_bytes());
        for value in values {
            push_lp(bytes, value);
        }
    }
}
