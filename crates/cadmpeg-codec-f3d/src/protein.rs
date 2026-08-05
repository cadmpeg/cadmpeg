// SPDX-License-Identifier: Apache-2.0
//! Schema-driven decoding of Protein `InstanceProperties` records.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Cursor;

use cadmpeg_core::CodecError;

use crate::bytes::take_lp_utf8_capped;

const PAGE_SIZE: usize = 0x88;
const RECORD_MARKER: &[u8] = b"\x80\x00\x01\x00";
const CONTINUATION_MARKER: &[u8] = b"\x80\x00\x00\x00";
const TERMINAL_MARKER: &[u8] = b"\xff\xff\xff\xff";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Carrier {
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
    Reference,
    TextureUri,
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

/// One typed property decoded according to its packaged schema.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DecodedProperty {
    pub(crate) value: PropertyValue,
    pub(crate) connections: Vec<String>,
}

/// A schema-defined Protein property value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PropertyValue {
    Boolean(bool),
    Integer(u32),
    Float(f64),
    Distance {
        unit: u32,
        value: f64,
    },
    String(String),
    Color([f64; 4]),
    Reference,
    TextureUri(Vec<String>),
    /// A member declared `allowmultiplevalues="true"` on a carrier other than
    /// `TextureURI`: a `u32` count followed by that many carrier values.
    Multiple(Vec<PropertyValue>),
}

/// One paged Protein instance record.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DecodedRecord {
    pub(crate) schema: String,
    pub(crate) guid: String,
    pub(crate) base: String,
    /// Library holding the preset this asset instantiates: a GUID for a shipped
    /// library, a path for a user library.
    pub(crate) asset_lib_id: String,
    pub(crate) properties: BTreeMap<String, DecodedProperty>,
}

/// Decode every `InstanceProperties` record in the paged `instance` stream
/// using the schemas packaged in the same Protein archive. Records whose value
/// block cannot be consumed are dropped; page framing keeps the remaining
/// records decodable.
pub(crate) fn decode(protein: &[u8], instance: &[u8]) -> Result<Vec<DecodedRecord>, CodecError> {
    let schemas = schemas(protein)?;
    let Some(pages) = paged_records(instance) else {
        return Ok(Vec::new());
    };
    let mut records = Vec::new();
    for record in pages {
        if let Ok(Some(record)) = decode_record(&record, &schemas) {
            records.push(record);
        }
    }
    Ok(records)
}

/// Split a paged `InstanceProperties` stream into logical records.
///
/// The stream is a 16-byte header followed by fixed [`PAGE_SIZE`] pages. A page
/// whose bytes 4..8 hold [`RECORD_MARKER`] opens a record, [`CONTINUATION_MARKER`]
/// extends it, and a page opening with [`TERMINAL_MARKER`] closes it and carries
/// the used byte count as a `u16` at offset 4. Every record is returned with the
/// opening marker restored so record offsets match the on-page layout.
fn paged_records(bytes: &[u8]) -> Option<Vec<Vec<u8>>> {
    if bytes.len() < 16 + PAGE_SIZE
        || u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?) as usize != PAGE_SIZE
        || !(bytes.len() - 16).is_multiple_of(PAGE_SIZE)
    {
        return None;
    }
    let mut records = Vec::new();
    let mut current: Option<Vec<u8>> = None;
    for page in bytes[16..].chunks_exact(PAGE_SIZE) {
        if page.get(4..8) == Some(RECORD_MARKER) {
            if let Some(record) = current.take() {
                records.push(record);
            }
            let mut record = RECORD_MARKER.to_vec();
            record.extend_from_slice(&page[8..]);
            current = Some(record);
        } else if page.get(4..8) == Some(CONTINUATION_MARKER) {
            current.as_mut()?.extend_from_slice(&page[8..]);
        } else if page.get(0..4) == Some(TERMINAL_MARKER) {
            let used = u16::from_le_bytes(page.get(4..6)?.try_into().ok()?) as usize;
            let mut record = current.take()?;
            record.extend_from_slice(page.get(8..8 + used)?);
            records.push(record);
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
pub(crate) fn has_schemas(protein: &[u8]) -> bool {
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
        CodecError::Malformed(format!("cannot open nested Protein ZIP: {error}"))
    })?;
    let mut schemas = HashMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            CodecError::Malformed(format!("cannot read nested Protein entry: {error}"))
        })?;
        if !is_schema_entry(entry.name()) {
            continue;
        }
        let size = entry.size();
        let name = entry.name().to_owned();
        let bytes = crate::container::read_entry_bounded(&mut entry, size, &name)?;
        let xml = std::str::from_utf8(&bytes).map_err(|error| {
            CodecError::Malformed(format!("Protein schema {name} is not UTF-8: {error}"))
        })?;
        let document = roxmltree::Document::parse(xml).map_err(|error| {
            CodecError::Malformed(format!("Protein schema {name} is malformed XML: {error}"))
        })?;
        let root = document.root_element();
        let uid = root
            .children()
            .find(|node| node.has_tag_name("UID"))
            .and_then(|node| node.attribute("val"))
            .ok_or_else(|| CodecError::Malformed(format!("Protein schema {name} has no UID")))?;
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
            if carrier == Carrier::Float && node.attribute("unit").is_some() {
                carrier = Carrier::UnitFloat;
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
            return Err(CodecError::Malformed(format!(
                "Protein archive defines schema {uid} more than once"
            )));
        }
    }
    Ok(schemas)
}

fn carrier(name: &str) -> Option<Carrier> {
    Some(match name {
        "Boolean" => Carrier::Boolean,
        "Integer" => Carrier::Integer,
        "Choice" => Carrier::Choice,
        "Float" => Carrier::Float,
        "Distance" => Carrier::Distance,
        "String" => Carrier::String,
        "Uuid" => Carrier::Uuid,
        "URL" => Carrier::Url,
        "Color" => Carrier::Color,
        "Reference" => Carrier::Reference,
        "TextureURI" => Carrier::TextureUri,
        _ => return None,
    })
}

fn property_closure(
    name: &str,
    schemas: &HashMap<String, Schema>,
    active: &mut BTreeSet<String>,
) -> Result<BTreeMap<String, Property>, CodecError> {
    if !active.insert(name.to_owned()) {
        return Err(CodecError::Malformed(format!(
            "Protein schema inheritance contains a cycle at {name}"
        )));
    }
    let schema = schemas.get(name).ok_or_else(|| {
        CodecError::Malformed(format!("Protein instance references absent schema {name}"))
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
        let value = read_property(record, &mut at, &property, &id).map_err(|error| {
            CodecError::Malformed(format!(
                "Protein {schema} instance {guid} property {id} at {property_at}..{at}/{}: {error}",
                record.len()
            ))
        })?;
        let connections = if property.connectable || property.carrier == Carrier::Reference {
            read_connections(record, &mut at).map_err(|error| {
                CodecError::Malformed(format!(
                    "Protein {schema} instance {guid} property {id} connection at {at}/{}: {error}",
                    record.len()
                ))
            })?
        } else {
            Vec::new()
        };
        values.insert(id, DecodedProperty { value, connections });
    }
    if at != record.len() {
        return Err(CodecError::Malformed(format!(
            "Protein {schema} instance {guid} consumed {at} of {} record bytes",
            record.len()
        )));
    }
    Ok(Some(DecodedRecord {
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
    property: &Property,
    id: &str,
) -> Result<PropertyValue, CodecError> {
    // A `TextureURI` carries its own kind byte in place of a count, so its
    // `allowmultiplevalues="true"` declaration adds no count prefix.
    if !property.multiple || property.carrier == Carrier::TextureUri {
        return read_value(bytes, at, property.carrier, id);
    }
    let count = read_count(bytes, at, id)?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(read_value(bytes, at, property.carrier, id)?);
    }
    Ok(PropertyValue::Multiple(values))
}

/// A `TextureURI` value: a kind byte, then either a counted list of paths
/// (kind 0, used for cloud resource references) or a single path (kind 1).
fn read_texture_uri(bytes: &[u8], at: &mut usize, id: &str) -> Result<PropertyValue, CodecError> {
    let malformed = || CodecError::Malformed(format!("Protein property {id} is truncated"));
    let kind = take::<1>(bytes, at).ok_or_else(malformed)?[0];
    if kind == 1 {
        return Ok(PropertyValue::TextureUri(vec![take_lp_utf8_capped(
            bytes, at, 1_048_576,
        )
        .ok_or_else(malformed)?]));
    }
    if kind != 0 {
        return Err(CodecError::Malformed(format!(
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
    let raw = take(bytes, at)
        .ok_or_else(|| CodecError::Malformed(format!("Protein property {id} is truncated")))?;
    let count = usize::try_from(u32::from_le_bytes(raw))
        .map_err(|_| CodecError::Malformed("Protein value count exceeds usize".into()))?;
    if count > 1_024 {
        return Err(CodecError::Malformed(format!(
            "Protein property {id} has implausible value count {count}"
        )));
    }
    Ok(count)
}

fn read_value(
    bytes: &[u8],
    at: &mut usize,
    carrier: Carrier,
    id: &str,
) -> Result<PropertyValue, CodecError> {
    let malformed = || CodecError::Malformed(format!("Protein property {id} is truncated"));
    Ok(match carrier {
        Carrier::Boolean => {
            PropertyValue::Boolean(take::<1>(bytes, at).ok_or_else(malformed)?[0] != 0)
        }
        Carrier::Integer | Carrier::Choice => {
            PropertyValue::Integer(u32::from_le_bytes(take(bytes, at).ok_or_else(malformed)?))
        }
        Carrier::Float => {
            PropertyValue::Float(f64::from_le_bytes(take(bytes, at).ok_or_else(malformed)?))
        }
        Carrier::UnitFloat => {
            take::<4>(bytes, at).ok_or_else(malformed)?;
            PropertyValue::Float(f64::from_le_bytes(take(bytes, at).ok_or_else(malformed)?))
        }
        Carrier::Distance => PropertyValue::Distance {
            unit: u32::from_le_bytes(take(bytes, at).ok_or_else(malformed)?),
            value: f64::from_le_bytes(take(bytes, at).ok_or_else(malformed)?),
        },
        Carrier::String | Carrier::Uuid | Carrier::Url => {
            PropertyValue::String(take_lp_utf8_capped(bytes, at, 1_048_576).ok_or_else(malformed)?)
        }
        Carrier::Color => {
            let mut rgba = [0.0; 4];
            for value in &mut rgba {
                *value = f64::from_le_bytes(take(bytes, at).ok_or_else(malformed)?);
            }
            PropertyValue::Color(rgba)
        }
        Carrier::Reference => PropertyValue::Reference,
        Carrier::TextureUri => return read_texture_uri(bytes, at, id),
    })
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
        return Err(CodecError::Malformed(format!(
            "Protein property has invalid connection flag {}",
            present[0]
        )));
    }
    let kind = take::<1>(bytes, at).ok_or_else(|| {
        CodecError::Malformed("Protein property connection kind is truncated".into())
    })?;
    if kind != [1] {
        return Err(CodecError::Malformed(format!(
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
                read_value(&bare, &mut at, Carrier::Color, id).unwrap(),
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
            properties["a_color"],
            DecodedProperty {
                value: PropertyValue::Color([0.1, 0.2, 0.3, 1.0]),
                connections: vec!["first-guid".into(), "second-guid".into()],
            }
        );
        assert_eq!(
            properties["b_distance"].value,
            PropertyValue::Distance {
                unit: 0x2016,
                value: 2.5,
            }
        );
        assert_eq!(
            properties["c_uri"].value,
            PropertyValue::TextureUri(vec![
                "cloud/resource/one".into(),
                "cloud/resource/two".into(),
            ])
        );
        assert_eq!(properties["d_unit_float"].value, PropertyValue::Float(4.5));
        assert_eq!(
            properties["e_reference"].connections,
            vec!["reference-guid"]
        );
        assert_eq!(
            properties["f_profile"].value,
            PropertyValue::Multiple(vec![PropertyValue::Float(0.25), PropertyValue::Float(0.75)])
        );
        assert_eq!(
            properties["metadata_still_serializes"].value,
            PropertyValue::String("Comments".into())
        );
        // `public="false"` does not suppress serialization.
        assert_eq!(properties["revision"].value, PropertyValue::Integer(1));
        assert_eq!(
            properties["swatch"].value,
            PropertyValue::String("Swatch-Torus".into())
        );
        assert_eq!(
            properties["ExchangeGUID"].value,
            PropertyValue::String(String::new())
        );
        // Consumed as the fourth record header string.
        assert!(!properties.contains_key("AssetLibID"));
        assert!(!properties.contains_key("renamed_color"));
        assert!(!properties.contains_key("ignored_readonly"));
        assert!(!properties.contains_key("ignored_definition"));
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
            properties["texture_MapChannel"].value,
            PropertyValue::Integer(1)
        );
        assert_eq!(
            properties["texture_MapChannel_UVWSource_Advanced"].value,
            PropertyValue::Integer(0)
        );
        assert_eq!(
            properties["swatch"].value,
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
        let records = paged_records(&stream).expect("stream is paged");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0], [RECORD_MARKER, &first].concat());
        assert_eq!(records[1], [RECORD_MARKER, &second].concat());
        assert!(stream.len() > 16 + 3 * PAGE_SIZE, "record one spans pages");

        assert!(paged_records(&[]).is_none());
        let mut truncated = stream.clone();
        truncated.truncate(16 + PAGE_SIZE + 1);
        assert!(paged_records(&truncated).is_none());
    }

    /// Lay records out as `InstanceProperties.bin` does: a 16-byte stream header,
    /// then a marker page, continuation pages, and a terminal page per record.
    fn paged_stream(records: &[&[u8]]) -> Vec<u8> {
        const BODY: usize = PAGE_SIZE - 8;
        let mut out = (PAGE_SIZE as u32).to_le_bytes().to_vec();
        out.resize(16, 0);
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
        let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
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
