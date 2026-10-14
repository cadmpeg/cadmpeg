// SPDX-License-Identifier: Apache-2.0
//! Schema-driven decoding of Protein `InstanceProperties` records.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Cursor, Read};

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

/// Neutral material and texture projection.
pub mod appearance;

/// Paged logical-record framing.
pub mod framing;

/// Decoded property carriers and their serialized representation.
pub mod property;
use cadmpeg_ir::scalar::FiniteReal;
use property::{DecodedProperty, PropertyContent, PropertyValue, RepeatedValues};

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
const MAX_RECOVERY_VALUES: u64 = 1_024;

fn take_lp_utf8_capped(bytes: &[u8], at: &mut usize, max: usize) -> Option<String> {
    let count = usize::try_from(View::u32_le_at(bytes, *at)?).ok()?;
    let start = at.checked_add(4)?;
    *at = start;
    if count > max {
        return None;
    }
    let end = start.checked_add(count)?;
    let value = std::str::from_utf8(bytes.get(start..end)?).ok()?.to_owned();
    *at = end;
    Some(value)
}

fn read_entry_bounded(
    ctx: &DecodeContext<'_>,
    entry: &mut impl Read,
    declared_size: u64,
) -> Result<Vec<u8>, CodecError> {
    if declared_size > MAX_SCHEMA_BYTES {
        return Err(schema_too_large(declared_size));
    }
    let mut writer = ctx.begin_expand(ExpandSpec::Exact(declared_size))?;
    let mut limited = entry.take(MAX_SCHEMA_BYTES + 1);
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let read = limited.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        let count = cadmpeg_core::decode::u64_from_index(read);
        let total = writer
            .written()
            .checked_add(count)
            .ok_or_else(|| CodecError::malformed("Protein schema byte count overflows"))?;
        if total > MAX_SCHEMA_BYTES {
            return Err(schema_too_large(total));
        }
        writer.write(&chunk[..read])?;
    }
    writer.finalize_owned()
}

/// A schema document past the fixed schema cap cannot be decoded.
fn schema_too_large(bytes: u64) -> CodecError {
    CodecError::NotImplemented(format!(
        "Protein schema of {bytes} bytes exceeds the {MAX_SCHEMA_BYTES}-byte schema cap"
    ))
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
                return Err(schema_too_large(entry.uncompressed_size));
            }
            let xml = archive.open(ctx, &entry.name)?;
            parse_schema_document(ctx, &entry.name, xml.window(), &mut schemas)?;
        }
        if schemas.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            schemas,
            properties: HashMap::new(),
        }))
    }

    /// Parse schemas for source-retaining export edits.
    pub fn load_for_edit(protein: &[u8]) -> Result<Self, CodecError> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(protein, &arena, &DecodePolicy::default())?;
        let schemas = schemas(&ctx, protein)?;
        Ok(Self {
            schemas,
            properties: HashMap::new(),
        })
    }

    fn properties_for(
        &mut self,
        ctx: &DecodeContext<'_>,
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
    ctx: &DecodeContext<'_>,
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
        depth_guards.push(ctx.enter_nested("Protein schema inheritance")?);
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
    decode_frames_admitted(ctx, &mut catalog, frames.frames())
}

/// Decode already framed records for source-retaining export edits.
pub fn decode_frames_for_edit(
    protein: &[u8],
    frames: &[framing::RecordFrame],
) -> Result<DecodeOutcome, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(protein, &arena, &DecodePolicy::default())?;
    let mut catalog = SchemaCatalog {
        schemas: schemas(&ctx, protein)?,
        properties: HashMap::new(),
    };
    decode_frames(&ctx, &mut catalog, frames)
}

/// Decode frames admitted by the caller against one parsed schema catalog.
pub fn decode_frames_admitted(
    ctx: &DecodeContext<'_>,
    catalog: &mut SchemaCatalog,
    frames: &[framing::RecordFrame],
) -> Result<DecodeOutcome, CodecError> {
    decode_frames(ctx, catalog, frames)
}

fn decode_frames(
    ctx: &DecodeContext<'_>,
    catalog: &mut SchemaCatalog,
    frames: &[framing::RecordFrame],
) -> Result<DecodeOutcome, CodecError> {
    let mut outcome = DecodeOutcome::default();
    for (ordinal, frame) in frames.iter().enumerate() {
        let ordinal = cadmpeg_core::decode::u64_from_index(ordinal);
        match decode_record(ctx, frame.bytes(), catalog, ordinal, frame.logical_offset()) {
            Ok(Some(record)) => {
                outcome.records.push(record);
            }
            Ok(None) => {
                outcome.rejected.push(RejectedRecord {
                    ordinal,
                    detail: "Protein instance record header is malformed".to_owned(),
                });
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                outcome.rejected.push(RejectedRecord {
                    ordinal,
                    detail: error.to_string(),
                });
            }
        }
    }
    Ok(outcome)
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

fn schemas(ctx: &DecodeContext<'_>, protein: &[u8]) -> Result<HashMap<String, Schema>, CodecError> {
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
        let bytes = read_entry_bounded(ctx, &mut entry, size)?;
        parse_schema_document(ctx, &name, &bytes, &mut schemas)?;
    }
    Ok(schemas)
}

fn parse_schema_document(
    ctx: &DecodeContext<'_>,
    name: &str,
    bytes: &[u8],
    schemas: &mut HashMap<String, Schema>,
) -> Result<(), CodecError> {
    let xml = std::str::from_utf8(bytes).map_err(|error| {
        CodecError::malformed(format_args!("Protein schema {name} is not UTF-8: {error}"))
    })?;
    let admitted_document = ctx
        .parse_xml(xml, "Protein schema XML tree")
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            CodecError::malformed(format_args!(
                "Protein schema {name} is malformed XML: {error}"
            ))
        })?;
    let document = admitted_document.document();
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
        if schema.properties.contains_key(id) {
            return Err(CodecError::malformed(format_args!(
                "Protein schema {uid} declares property {id} more than once"
            )));
        }
        schema.properties.insert(id.to_owned(), property);
    }
    if schemas.contains_key(uid) {
        return Err(CodecError::malformed(format_args!(
            "Protein archive defines schema {uid} more than once"
        )));
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
    ctx: &DecodeContext<'_>,
    record: &[u8],
    catalog: &mut SchemaCatalog,
    ordinal: u64,
    logical_offset: usize,
) -> Result<Option<DecodedRecord>, CodecError> {
    if !record.starts_with(RECORD_MARKER) {
        return Ok(None);
    }
    let mut at = RECORD_MARKER.len();
    let Some(schema) = take_lp_utf8_capped(record, &mut at, 1_048_576) else {
        return Ok(None);
    };
    let Some(guid) = take_lp_utf8_capped(record, &mut at, 1_048_576) else {
        return Ok(None);
    };
    let Some(base) = take_lp_utf8_capped(record, &mut at, 1_048_576) else {
        return Ok(None);
    };
    // The fourth header string is `AssetLibID`, the first member of
    // `CommonSchema` in serialization order. It is carried in the record header
    // rather than in the value block, so `instance_property_serializes` drops
    // the member there.
    let Some(asset_lib_id) = take_lp_utf8_capped(record, &mut at, 1_048_576) else {
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
                let targets = read_connections(record, &mut at)
                    .map_err(|error| connection_error(error, at))?;
                match count {
                    Some(count) => match std::num::NonZeroUsize::new(count) {
                        Some(count) => PropertyContent::MultipleReferences { count, targets },
                        None => PropertyContent::Value {
                            value: PropertyValue::Multiple(RepeatedValues::default()),
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
                let value = read_property(record, &mut at, *layout, id)
                    .map_err(|error| value_error(error, at))?;
                let connections = (*connectable)
                    .then(|| read_connections(record, &mut at))
                    .transpose()
                    .map_err(|error| connection_error(error, at))?
                    .unwrap_or_default();
                PropertyContent::Value { value, connections }
            }
        };
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
    bytes: &[u8],
    at: &mut usize,
    layout: ValueLayout,
    id: &str,
) -> Result<PropertyValue, CodecError> {
    match layout {
        ValueLayout::Single(carrier) => read_value(bytes, at, carrier, id),
        // TextureURI owns its kind byte and optional count; the schema's
        // multiple-value declaration does not add another count prefix.
        ValueLayout::TextureUri => read_texture_uri(bytes, at, id),
        ValueLayout::Multiple(carrier) => {
            let count = read_count(bytes, at, id)?;
            let mut values = Vec::new();
            for _ in 0..count {
                values.push(read_value(bytes, at, carrier, id)?);
            }
            Ok(PropertyValue::Multiple(
                values.try_into().map_err(CodecError::malformed)?,
            ))
        }
    }
}

/// A `TextureURI` value: a kind byte, then either a counted list of paths
/// (kind 0, used for cloud resource references) or a single path (kind 1).
fn read_texture_uri(bytes: &[u8], at: &mut usize, id: &str) -> Result<PropertyValue, CodecError> {
    let malformed = || CodecError::malformed(format_args!("Protein property {id} is truncated"));
    let kind = take::<1>(bytes, at).ok_or_else(malformed)?[0];
    if kind == 1 {
        return Ok(PropertyValue::TextureUri(vec![take_lp_utf8_capped(
            bytes, at, 1_048_576,
        )
        .ok_or_else(malformed)?]));
    }
    if kind != 0 {
        return Err(CodecError::malformed(format_args!(
            "Protein TextureURI property {id} has invalid kind {kind}"
        )));
    }
    let count = read_count(bytes, at, id)?;
    let mut paths = Vec::new();
    for _ in 0..count {
        paths.push(take_lp_utf8_capped(bytes, at, 1_048_576).ok_or_else(malformed)?);
    }
    Ok(PropertyValue::TextureUri(paths))
}

fn read_count(bytes: &[u8], at: &mut usize, id: &str) -> Result<usize, CodecError> {
    let count = usize::try_from(read_u32_le(bytes, at).ok_or_else(|| {
        CodecError::malformed(format_args!("Protein property {id} is truncated"))
    })?)
    .map_err(|_| CodecError::Malformed("Protein value count exceeds usize".into()))?;
    if cadmpeg_core::decode::u64_from_index(count) > MAX_RECOVERY_VALUES {
        return Err(CodecError::malformed(format_args!(
            "Protein property {id} declares {count} values; at most {MAX_RECOVERY_VALUES} are decoded"
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
        ValueCarrier::String => {
            PropertyValue::String(take_lp_utf8_capped(bytes, at, 1_048_576).ok_or_else(malformed)?)
        }
        ValueCarrier::Color => {
            let mut rgba = [FiniteReal::ZERO; 4];
            for value in &mut rgba {
                *value = finite_value(read_f64_le(bytes, at).ok_or_else(malformed)?, id)?;
            }
            PropertyValue::Color(rgba)
        }
    })
}

fn finite_value(value: f64, id: &str) -> Result<FiniteReal, CodecError> {
    FiniteReal::new(value)
        .ok_or_else(|| CodecError::malformed(format_args!("Protein property {id} is not finite")))
}

/// The connection block that follows every connectable member and every
/// `Reference`: a presence byte, then a kind byte, a `u32` count, and that many
/// length-prefixed connected-asset GUIDs.
fn read_connections(bytes: &[u8], at: &mut usize) -> Result<Vec<String>, CodecError> {
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
    let mut connections = Vec::new();
    for _ in 0..count {
        connections.push(take_lp_utf8_capped(bytes, at, 1_048_576).ok_or_else(|| {
            CodecError::Malformed("Protein property connection GUID is truncated".into())
        })?);
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
        FiniteReal, RepeatedValues, ValueCarrier, CONTINUATION_MARKER, PAGE_SIZE, RECORD_MARKER,
        STREAM_HEADER_LEN, TERMINAL_MARKER,
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

    fn with_service_context<T>(
        bytes: &[u8],
        use_context: impl FnOnce(&DecodeContext<'_>) -> T,
    ) -> T {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
            .expect("fixture fits service profile");
        use_context(&ctx)
    }

    #[test]
    fn schema_edit_reader_reads_the_declared_bytes() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(b"xml", &arena, &DecodePolicy::service()).expect("root");
        assert_eq!(
            super::read_entry_bounded(&ctx, &mut Cursor::new(b"xml"), 3)
                .expect("service admission"),
            b"xml"
        );
    }

    #[test]
    fn schema_size_cap_is_unsupported_and_expansion_limit_refuses() {
        with_service_context(&[], |ctx| {
            let error =
                super::read_entry_bounded(ctx, &mut Cursor::new(b""), super::MAX_SCHEMA_BYTES + 1)
                    .expect_err("local byte ceiling");
            assert!(matches!(error, CodecError::NotImplemented(_)), "{error:?}");
        });
        let mut policy = DecodePolicy::service();
        policy.limits.max_decompressed_bytes_per_expand = 2;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(b"xml", &arena, &policy).expect("root");
        let error = super::read_entry_bounded(&ctx, &mut Cursor::new(b"xml"), 3)
            .expect_err("caller expansion limit");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::DecompressedBytes)
        );
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
        policy.limits.max_decompressed_bytes_per_expand =
            cadmpeg_core::decode::u64_from_index(xml.len()) - 1;
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
            super::resolve_inheritance(&ctx, &schemas, &mut std::collections::HashMap::new(), "AChild"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RecursionDepth
        ));
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("fixture fits service profile");
        assert!(super::resolve_inheritance(
            &ctx,
            &schemas,
            &mut std::collections::HashMap::new(),
            "AChild"
        )
        .is_ok());
    }

    #[test]
    fn edit_decode_uses_default_inheritance_limit() {
        fn archive_with_depth(depth: usize) -> Vec<u8> {
            let entries = (0..depth)
                .map(|index| {
                    let name = format!("Schemas/Schema{index}/Schema.xml");
                    let base = index
                        .checked_sub(1)
                        .map_or_else(String::new, |base| format!("<Base val=\"Schema{base}\"/>"));
                    let xml = format!("<Schema><UID val=\"Schema{index}\"/>{base}</Schema>");
                    (name, xml)
                })
                .collect::<Vec<_>>();
            let borrowed = entries
                .iter()
                .map(|(name, xml)| (name.as_str(), xml.as_str()))
                .collect::<Vec<_>>();
            schema_archive(&borrowed)
        }

        fn frames_for_schema(name: &str) -> Vec<framing::RecordFrame> {
            let mut record = Vec::new();
            for value in [name, "guid", "base", ""] {
                push_lp(&mut record, value);
            }
            framing::record_frames_for_edit(&paged_stream(&[&record]))
                .expect("fixture framing is valid")
        }

        let normal =
            super::decode_frames_for_edit(&archive_with_depth(1), &frames_for_schema("Schema0"))
                .expect("one schema fits the default policy");
        assert_eq!(normal.records.len(), 1, "{:?}", normal.rejected);
        assert!(matches!(
            super::decode_frames_for_edit(&archive_with_depth(257), &frames_for_schema("Schema256")),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RecursionDepth
        ));
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
    fn duplicate_local_protein_properties_are_rejected() {
        let xml = br#"<Schema><UID val="Simple"/><Float id="value"/><String id="value"/></Schema>"#;
        with_service_context(xml, |ctx| {
            let error = super::parse_schema_document(
                ctx,
                "schema",
                xml,
                &mut std::collections::HashMap::new(),
            )
            .expect_err("local ids must be unique");
            assert!(
                matches!(error, CodecError::Malformed(message) if message == "Protein schema Simple declares property value more than once")
            );
        });
    }

    #[test]
    fn schema_archive_size_ceiling_is_unsupported() {
        let mut bytes = schema_archive(&[(
            "Schemas/SimpleSchema.xml",
            "<Schema><UID val=\"Simple\"/></Schema>",
        )]);
        let size = u32::try_from(super::MAX_SCHEMA_BYTES + 1)
            .expect("local ceiling fits ZIP32")
            .to_le_bytes();
        let central = bytes
            .windows(4)
            .position(|bytes| bytes == b"PK\x01\x02")
            .expect("central header");
        bytes[central + 24..central + 28].copy_from_slice(&size);
        bytes[22..26].copy_from_slice(&size);
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        let error = super::SchemaCatalog::load(&ctx, root)
            .err()
            .expect("local size ceiling");
        assert!(matches!(error, CodecError::NotImplemented(_)), "{error:?}");
    }

    #[test]
    fn large_protein_lists_past_the_value_cap_are_malformed() {
        let mut repeated = 1_025_u32.to_le_bytes().to_vec();
        repeated.extend(std::iter::repeat_n(0_u8, 1_025 * 4));
        let mut paths = vec![0_u8];
        paths.extend_from_slice(&repeated);
        let mut connections = vec![1_u8, 1];
        connections.extend_from_slice(&repeated);
        let over_cap = |error: CodecError| matches!(&error, CodecError::Malformed(message) if message.contains("at most 1024"));
        assert!(over_cap(
            super::read_property(
                &repeated,
                &mut 0,
                super::ValueLayout::Multiple(ValueCarrier::Integer),
                "values",
            )
            .expect_err("value cap")
        ));
        assert!(over_cap(
            read_texture_uri(&paths, &mut 0, "paths").expect_err("URI value cap")
        ));
        assert!(over_cap(
            read_connections(&connections, &mut 0).expect_err("connection value cap")
        ));
    }

    #[test]
    fn a_record_past_the_value_cap_is_rejected_and_decoding_continues() {
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
        let mut over = Vec::new();
        let mut within = Vec::new();
        for value in ["Simple", "guid", "base", ""] {
            push_lp(&mut over, value);
            push_lp(&mut within, value);
        }
        over.extend_from_slice(&1_025_u32.to_le_bytes());
        over.extend(std::iter::repeat_n(0_u8, 1_025 * 4));
        within.extend_from_slice(&1_u32.to_le_bytes());
        within.extend_from_slice(&7_u32.to_le_bytes());
        let stream = paged_stream(&[&over, &within]);
        let frames = framing::record_frames_for_edit(&stream).expect("fixture framing is valid");
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &DecodePolicy::service())
            .expect("stream fits service profile");
        let outcome = super::decode_frames_admitted(&ctx, &mut catalog, &frames)
            .expect("a capped record is rejected, not refused");
        assert_eq!(outcome.records.len(), 1);
        assert_eq!(outcome.rejected.len(), 1);
        assert_eq!(outcome.rejected[0].ordinal, 0);
        assert!(outcome.rejected[0].detail.contains("at most 1024"));
    }

    #[test]
    fn absent_schema_rejects_the_record() {
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
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &DecodePolicy::service())
            .expect("stream fits service profile");
        assert_eq!(
            super::decode_frames_admitted(&ctx, &mut catalog, &frames)
                .expect("an absent schema rejects only its record")
                .rejected
                .len(),
            1
        );
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
            {
                assert_eq!(
                    read_value(&bare, &mut at, ValueCarrier::Color, id).unwrap(),
                    PropertyValue::Color(rgba.map(|value| FiniteReal::new(value).expect("finite")))
                );
            }
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
        {
            assert_eq!(
                read_connections(&connections, &mut at).unwrap(),
                ["first-guid", "second-guid"]
            );
        }
        assert_eq!(at, connections.len());

        let mut at = 0;
        {
            assert!(read_connections(&[0], &mut at).unwrap().is_empty());
        }
        assert_eq!(at, 1);

        let mut counted = vec![0];
        counted.extend_from_slice(&1u32.to_le_bytes());
        push_lp(&mut counted, "cloud/resource/one");
        let mut at = 0;
        {
            assert_eq!(
                read_texture_uri(&counted, &mut at, "unifiedbitmap_Bitmap").unwrap(),
                PropertyValue::TextureUri(vec!["cloud/resource/one".into()])
            );
        }
        assert_eq!(at, counted.len());

        let mut single = vec![1];
        push_lp(&mut single, "local_bitmap.png");
        let mut at = 0;
        {
            assert_eq!(
                read_texture_uri(&single, &mut at, "unifiedbitmap_Bitmap").unwrap(),
                PropertyValue::TextureUri(vec!["local_bitmap.png".into()])
            );
        }
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
                match std::num::NonZeroUsize::new(cadmpeg_core::decode::index_from_u32(count)) {
                    Some(count) => PropertyContent::MultipleReferences {
                        count,
                        targets: vec!["target".into()],
                    },
                    None => PropertyContent::Value {
                        value: PropertyValue::Multiple(RepeatedValues::default()),
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
            PropertyValue::Color(
                [0.1, 0.2, 0.3, 1.0].map(|value| FiniteReal::new(value).expect("finite"))
            )
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
                value: FiniteReal::new(2.5).expect("finite"),
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
            PropertyValue::Float(FiniteReal::new(4.5).expect("finite"))
        );
        assert_eq!(
            properties["e_reference"].connections(),
            vec!["reference-guid"]
        );
        assert_eq!(
            properties["f_profile"].value().unwrap().clone(),
            PropertyValue::Multiple(
                vec![
                    PropertyValue::Float(FiniteReal::new(0.25).expect("finite")),
                    PropertyValue::Float(FiniteReal::new(0.75).expect("finite"))
                ]
                .try_into()
                .expect("same carrier")
            )
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
        let mut stream = u32::try_from(PAGE_SIZE)
            .expect("test page size fits u32")
            .to_le_bytes()
            .to_vec();
        stream.resize(STREAM_HEADER_LEN, 0);
        stream.extend_from_slice(TERMINAL_MARKER);
        stream.extend_from_slice(
            &u16::try_from(record.len())
                .expect("test record fits u16")
                .to_le_bytes(),
        );
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
        let mut out = u32::try_from(PAGE_SIZE)
            .expect("test page size fits u32")
            .to_le_bytes()
            .to_vec();
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
                header[4..6].copy_from_slice(
                    &u16::try_from(record.len())
                        .expect("test record fits u16")
                        .to_le_bytes(),
                );
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
                    header[4..6].copy_from_slice(
                        &u16::try_from(chunk.len())
                            .expect("test chunk fits u16")
                            .to_le_bytes(),
                    );
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
        bytes.extend_from_slice(
            &u32::try_from(value.len())
                .expect("test value fits u32")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(value.as_bytes());
    }

    fn push_connections(bytes: &mut Vec<u8>, values: &[&str]) {
        bytes.extend_from_slice(&[1, 1]);
        bytes.extend_from_slice(
            &u32::try_from(values.len())
                .expect("test count fits u32")
                .to_le_bytes(),
        );
        for value in values {
            push_lp(bytes, value);
        }
    }
}
