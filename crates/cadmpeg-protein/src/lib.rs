// SPDX-License-Identifier: Apache-2.0
//! Schema-driven decoding of Protein `InstanceProperties` records.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Cursor, Read};

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

/// Decoded property carriers and their serialized representation.
pub mod property;
use property::{DecodedProperty, PropertyContent, PropertyValue};

/// Byte-offset constants generated from `docs/layouts/protein.toml`.
pub(crate) mod layout;
use layout::{continuation_page, instance_stream_header, record_start_page, terminal_page};

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

fn take_lp_utf8_capped(bytes: &[u8], at: &mut usize, max: usize) -> Option<String> {
    let mut view = View::over_retained(bytes);
    view.seek(*at)?;
    let count = usize::try_from(view.u32_le()?).ok()?;
    *at = at.checked_add(4)?;
    if count > max {
        return None;
    }
    let end = at.checked_add(count)?;
    let value = std::str::from_utf8(bytes.get(*at..end)?).ok()?.to_owned();
    *at = end;
    Some(value)
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
                None,
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
    Choice,
    Float,
    UnitFloat,
    Distance,
    String,
    Uuid,
    Url,
    Color,
    TextureUri,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Carrier {
    Value(ValueCarrier),
    Reference,
}

#[derive(Clone, Debug)]
struct Property {
    carrier: Carrier,
    connectable: bool,
    multiple: bool,
}

#[derive(Debug, Default)]
struct Schema {
    base: Option<String>,
    properties: BTreeMap<String, Property>,
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

/// One exact logical record recovered from the `InstanceProperties` page
/// framing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordFrame {
    /// Byte offset in the dechunked logical stream.
    pub logical_offset: usize,
    /// Complete record bytes, including the opening marker.
    pub bytes: Vec<u8>,
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

/// Decode every valid `InstanceProperties` record in the paged `instance`
/// stream using the schemas packaged in the same Protein archive.
///
/// Use [`decode_detailed`] when rejected logical records must be accounted.
pub fn decode(protein: &[u8], instance: &[u8]) -> Result<Vec<DecodedRecord>, CodecError> {
    Ok(decode_detailed(protein, instance)?.records)
}

/// Decode every `InstanceProperties` record and account for each rejected
/// logical record without discarding later valid records.
pub fn decode_detailed(protein: &[u8], instance: &[u8]) -> Result<DecodeOutcome, CodecError> {
    let schemas = schemas(protein)?;
    let Some(frames) = record_frames(instance) else {
        return Err(CodecError::Malformed(
            "Protein InstanceProperties page framing is invalid".into(),
        ));
    };
    let mut outcome = DecodeOutcome::default();
    for (ordinal, frame) in frames.into_iter().enumerate() {
        let ordinal = u64::try_from(ordinal).map_err(|_| {
            CodecError::Malformed("Protein logical-record ordinal exceeds u64".into())
        })?;
        match decode_record(&frame.bytes, &schemas, ordinal, frame.logical_offset) {
            Ok(Some(record)) => outcome.records.push(record),
            Ok(None) => outcome.rejected.push(RejectedRecord {
                ordinal,
                detail: "Protein instance record header is malformed".into(),
            }),
            Err(error) => outcome.rejected.push(RejectedRecord {
                ordinal,
                detail: error.to_string(),
            }),
        }
    }
    Ok(outcome)
}

/// Split a paged `InstanceProperties` stream into logical records.
///
/// The stream is a [`STREAM_HEADER_LEN`]-byte header followed by fixed
/// [`PAGE_SIZE`] pages. A page whose bytes 4..8 hold [`RECORD_MARKER`] opens a
/// record, [`CONTINUATION_MARKER`] extends it, and a page opening with
/// [`TERMINAL_MARKER`] closes it and carries the used byte count as a `u16` at
/// offset 4. Every record is returned with the opening marker restored so
/// record offsets match the on-page layout.
pub fn record_frames(bytes: &[u8]) -> Option<Vec<RecordFrame>> {
    if bytes.len() < STREAM_HEADER_LEN + PAGE_SIZE
        || View::u32_le_at(bytes, instance_stream_header::DECLARED_SIZE)? as usize != PAGE_SIZE
        || !(bytes.len() - STREAM_HEADER_LEN).is_multiple_of(PAGE_SIZE)
    {
        return None;
    }
    let mut records = Vec::new();
    let mut current: Option<RecordFrame> = None;
    let mut logical_offset = 0usize;
    for page in bytes[STREAM_HEADER_LEN..].chunks_exact(PAGE_SIZE) {
        if page.get(record_start_page::MARKER..record_start_page::BODY) == Some(RECORD_MARKER) {
            if let Some(record) = current.take() {
                logical_offset = logical_offset.checked_add(record.bytes.len())?;
                records.push(record);
            }
            let mut frame = RecordFrame {
                logical_offset,
                bytes: RECORD_MARKER.to_vec(),
            };
            frame
                .bytes
                .extend_from_slice(&page[record_start_page::BODY..]);
            current = Some(frame);
        } else if page.get(continuation_page::MARKER..continuation_page::BODY)
            == Some(CONTINUATION_MARKER)
        {
            current
                .as_mut()?
                .bytes
                .extend_from_slice(&page[continuation_page::BODY..]);
        } else if page.get(terminal_page::MARKER..terminal_page::USED) == Some(TERMINAL_MARKER) {
            let used = View::u16_le_at(page, terminal_page::USED)? as usize;
            let mut frame = current.take().unwrap_or_else(|| RecordFrame {
                logical_offset,
                bytes: RECORD_MARKER.to_vec(),
            });
            frame
                .bytes
                .extend_from_slice(page.get(terminal_page::BODY..terminal_page::BODY + used)?);
            logical_offset = logical_offset.checked_add(frame.bytes.len())?;
            records.push(frame);
        } else {
            return None;
        }
    }
    if let Some(record) = current {
        records.push(record);
    }
    Some(records)
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
        let xml = std::str::from_utf8(&bytes).map_err(|error| {
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
            .ok_or_else(|| {
                CodecError::malformed(format_args!("Protein schema {name} has no UID"))
            })?;
        let mut schema = Schema::default();
        for node in root.children().filter(roxmltree::Node::is_element) {
            if node.has_tag_name("Base") {
                schema.base = node.attribute("val").map(str::to_owned);
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
            let Some(mut carrier) = carrier(node.tag_name().name()) else {
                continue;
            };
            if carrier == Carrier::Value(ValueCarrier::Float) && node.attribute("unit").is_some() {
                carrier = Carrier::Value(ValueCarrier::UnitFloat);
            }
            let Some(id) = node.attribute("id") else {
                continue;
            };
            schema.properties.insert(
                id.to_owned(),
                Property {
                    carrier,
                    connectable: node.attribute("allowconnectedassets").is_some(),
                    multiple: node.attribute("allowmultiplevalues") == Some("true"),
                },
            );
        }
        if schemas.insert(uid.to_owned(), schema).is_some() {
            return Err(CodecError::malformed(format_args!(
                "Protein archive defines schema {uid} more than once"
            )));
        }
    }
    Ok(schemas)
}

fn carrier(name: &str) -> Option<Carrier> {
    Some(match name {
        "Boolean" => Carrier::Value(ValueCarrier::Boolean),
        "Integer" => Carrier::Value(ValueCarrier::Integer),
        "Choice" => Carrier::Value(ValueCarrier::Choice),
        "Float" => Carrier::Value(ValueCarrier::Float),
        "Distance" => Carrier::Value(ValueCarrier::Distance),
        "String" => Carrier::Value(ValueCarrier::String),
        "Uuid" => Carrier::Value(ValueCarrier::Uuid),
        "URL" => Carrier::Value(ValueCarrier::Url),
        "Color" => Carrier::Value(ValueCarrier::Color),
        "Reference" => Carrier::Reference,
        "TextureURI" => Carrier::Value(ValueCarrier::TextureUri),
        _ => return None,
    })
}

fn property_closure(
    name: &str,
    schemas: &HashMap<String, Schema>,
    active: &mut BTreeSet<String>,
) -> Result<BTreeMap<String, Property>, CodecError> {
    if !active.insert(name.to_owned()) {
        return Err(CodecError::malformed(format_args!(
            "Protein schema inheritance contains a cycle at {name}"
        )));
    }
    let schema = schemas.get(name).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "Protein instance references absent schema {name}"
        ))
    })?;
    let mut properties = match schema.base.as_deref() {
        Some(base) => property_closure(base, schemas, active)?,
        None => BTreeMap::new(),
    };
    properties.extend(schema.properties.clone());
    active.remove(name);
    Ok(properties)
}

fn decode_record(
    record: &[u8],
    schemas: &HashMap<String, Schema>,
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
    let properties = property_closure(&schema, schemas, &mut BTreeSet::new())?;
    let mut values = BTreeMap::new();
    for (id, property) in properties {
        if !instance_property_serializes(&id) {
            continue;
        }
        let property_at = at;
        let value_offset = if !property.multiple
            && matches!(
                property.carrier,
                Carrier::Value(ValueCarrier::UnitFloat | ValueCarrier::Distance)
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
            CodecError::malformed(format_args!(
                "Protein {schema} instance {guid} property {id} at {property_at}..{at}/{}: {error}",
                record.len()
            ))
        };
        let connection_error = |error: CodecError, at: usize| {
            CodecError::malformed(format_args!(
                "Protein {schema} instance {guid} property {id} connection at {at}/{}: {error}",
                record.len()
            ))
        };
        let content = match property.carrier {
            Carrier::Reference => {
                let count = property
                    .multiple
                    .then(|| read_count(record, &mut at, &id))
                    .transpose()
                    .map_err(|error| value_error(error, at))?;
                let targets = read_connections(record, &mut at)
                    .map_err(|error| connection_error(error, at))?;
                match count {
                    Some(count) => PropertyContent::MultipleReferences { count, targets },
                    None => PropertyContent::Reference(targets),
                }
            }
            Carrier::Value(carrier) => {
                let value = read_property(record, &mut at, carrier, property.multiple, &id)
                    .map_err(|error| value_error(error, at))?;
                let connections = property
                    .connectable
                    .then(|| read_connections(record, &mut at))
                    .transpose()
                    .map_err(|error| connection_error(error, at))?;
                PropertyContent::Value { value, connections }
            }
        };
        values.insert(
            id,
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
    carrier: ValueCarrier,
    multiple: bool,
    id: &str,
) -> Result<PropertyValue, CodecError> {
    // A `TextureURI` carries its own kind byte in place of a count, so its
    // `allowmultiplevalues="true"` declaration adds no count prefix.
    if !multiple || carrier == ValueCarrier::TextureUri {
        return read_value(bytes, at, carrier, id);
    }
    let count = read_count(bytes, at, id)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(read_value(bytes, at, carrier, id)?);
    }
    Ok(PropertyValue::Multiple(values))
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
    let mut paths = Vec::with_capacity(count);
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
        ValueCarrier::Integer | ValueCarrier::Choice => {
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
        ValueCarrier::String | ValueCarrier::Uuid | ValueCarrier::Url => {
            PropertyValue::String(take_lp_utf8_capped(bytes, at, 1_048_576).ok_or_else(malformed)?)
        }
        ValueCarrier::Color => {
            let mut rgba = [0.0; 4];
            for value in &mut rgba {
                *value = finite_value(read_f64_le(bytes, at).ok_or_else(malformed)?, id)?;
            }
            PropertyValue::Color(rgba)
        }
        ValueCarrier::TextureUri => return read_texture_uri(bytes, at, id),
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
    let mut connections = Vec::with_capacity(count);
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

    use std::io::Write;

    use super::*;

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
                read_value(&bare, &mut at, ValueCarrier::Color, id).unwrap(),
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
            read_connections(&connections, &mut at).unwrap(),
            ["first-guid", "second-guid"]
        );
        assert_eq!(at, connections.len());

        let mut at = 0;
        assert!(read_connections(&[0], &mut at).unwrap().is_empty());
        assert_eq!(at, 1);

        let mut counted = vec![0];
        counted.extend_from_slice(&1u32.to_le_bytes());
        push_lp(&mut counted, "cloud/resource/one");
        let mut at = 0;
        assert_eq!(
            read_texture_uri(&counted, &mut at, "unifiedbitmap_Bitmap").unwrap(),
            PropertyValue::TextureUri(vec!["cloud/resource/one".into()])
        );
        assert_eq!(at, counted.len());

        let mut single = vec![1];
        push_lp(&mut single, "local_bitmap.png");
        let mut at = 0;
        assert_eq!(
            read_texture_uri(&single, &mut at, "unifiedbitmap_Bitmap").unwrap(),
            PropertyValue::TextureUri(vec!["local_bitmap.png".into()])
        );
        assert_eq!(at, single.len());
    }

    #[test]
    fn multiple_references_keep_one_connection_block_including_zero_values() {
        let protein = schema_archive(&[(
            "Schemas/References.xml",
            r#"<Schema><UID val="References"/><Reference id="targets" allowmultiplevalues="true"/></Schema>"#,
        )]);
        for count in [0_u32, 2] {
            let mut record = Vec::new();
            for value in ["References", "asset-guid", "Reference", ""] {
                push_lp(&mut record, value);
            }
            record.extend_from_slice(&count.to_le_bytes());
            push_connections(&mut record, &["target"]);
            let records = decode(&protein, &paged_stream(&[&record])).unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(
                records[0].properties["targets"].content,
                PropertyContent::MultipleReferences {
                    count: count as usize,
                    targets: vec!["target".into()]
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

        let records = decode(&protein, &paged_stream(&[&record])).expect("schema record decodes");
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

        let outcome = decode_detailed(&protein, &instance).expect("framed records decode");
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
        assert_eq!(decode(&protein, &instance).unwrap().len(), 2);
    }

    #[test]
    fn detailed_decode_rejects_invalid_page_framing() {
        let protein = schema_archive(&[(
            "Schemas/SimpleSchema.xml",
            r#"<Schema><UID val="SimpleSchema"/><String id="comment"/></Schema>"#,
        )]);
        let error = decode_detailed(&protein, &[0; 16])
            .expect_err("a header without any complete page is malformed");
        assert!(error.to_string().contains("page framing is invalid"));
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
        let records = decode(&protein, &paged_stream(&[&record])).expect("texture record decodes");
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
        let frames = record_frames(&stream).expect("stream is paged");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].logical_offset, 0);
        assert_eq!(frames[0].bytes, [RECORD_MARKER, &first].concat());
        assert_eq!(frames[1].logical_offset, frames[0].bytes.len());
        assert_eq!(frames[1].bytes, [RECORD_MARKER, &second].concat());
        assert!(stream.len() > 16 + 3 * PAGE_SIZE, "record one spans pages");

        assert!(record_frames(&[]).is_none());
        let mut truncated = stream.clone();
        truncated.truncate(16 + PAGE_SIZE + 1);
        assert!(record_frames(&truncated).is_none());
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

        let frames = record_frames(&stream).expect("standalone terminal page");
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].logical_offset, 0);
        assert_eq!(frames[0].bytes, [RECORD_MARKER, &record].concat());
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
            assert!(
                record.len() >= BODY,
                "a record fills at least one page body"
            );
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
