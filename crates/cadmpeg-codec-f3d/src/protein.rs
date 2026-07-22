// SPDX-License-Identifier: Apache-2.0
//! Schema-driven decoding of Protein `InstanceProperties` records.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Cursor;

use cadmpeg_ir::codec::CodecError;

use crate::bytes::take_lp_utf8_capped;

const RECORD_MARKER: &[u8] = b"\x80\x00\x01\x00";

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
    Color,
    Reference,
    TextureUri,
}

#[derive(Clone, Debug)]
struct Property {
    carrier: Carrier,
    connectable: bool,
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
    Distance { unit: u32, value: f64 },
    String(String),
    Color([f64; 4]),
    Reference,
    TextureUri(Vec<String>),
}

/// One paged Protein instance record.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DecodedRecord {
    pub(crate) schema: String,
    pub(crate) guid: String,
    pub(crate) base: String,
    pub(crate) properties: BTreeMap<String, DecodedProperty>,
}

/// Decode every supported `InstanceProperties` record using the schemas
/// packaged in the same Protein archive. A record whose schema-specific value
/// block cannot be consumed is isolated at the next paged-record marker.
pub(crate) fn decode(protein: &[u8], logical: &[u8]) -> Result<Vec<DecodedRecord>, CodecError> {
    let schemas = schemas(protein)?;
    let starts = logical
        .windows(RECORD_MARKER.len())
        .enumerate()
        .filter_map(|(offset, marker)| (marker == RECORD_MARKER).then_some(offset))
        .collect::<Vec<_>>();
    let mut records = Vec::new();
    for (ordinal, start) in starts.iter().copied().enumerate() {
        let end = starts.get(ordinal + 1).copied().unwrap_or(logical.len());
        if let Ok(Some(record)) = decode_record(&logical[start..end], &schemas) {
            records.push(record);
        }
    }
    Ok(records)
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
                || node.attribute("metadata") == Some("true")
                || node.attribute("public") == Some("false")
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
    let Some(_) = take_lp_utf8_capped(record, &mut at, 1_048_576) else {
        return Ok(None);
    };
    if schema == "PhysMatSchema"
        || schema.starts_with("Structural")
        || schema.starts_with("Thermal")
    {
        return Ok(Some(DecodedRecord {
            schema,
            guid,
            base,
            properties: BTreeMap::new(),
        }));
    }
    let properties = property_closure(&schema, schemas, &mut BTreeSet::new())?;
    let mut values = BTreeMap::new();
    for (id, property) in properties {
        if !instance_property_serializes(&schema, &id) {
            continue;
        }
        if id == "surface_albedo" && choice_at(record, at).is_some_and(|value| value <= 2) {
            take::<4>(record, &mut at).ok_or_else(|| {
                CodecError::Malformed("Protein surface albedo prelude is truncated".into())
            })?;
        }
        if id == "texture_RealWorldOffsetX" {
            take::<4>(record, &mut at).ok_or_else(|| {
                CodecError::Malformed("Protein texture mapping prelude is truncated".into())
            })?;
        }
        let property_at = at;
        let value = read_value(record, &mut at, property.carrier, &id, property.connectable)
            .map_err(|error| {
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
    Ok(Some(DecodedRecord {
        schema,
        guid,
        base,
        properties: values,
    }))
}

fn choice_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn instance_property_serializes(schema: &str, id: &str) -> bool {
    match id {
        "ExchangeGUID" => !matches!(schema, "BumpMapSchema" | "PrismMetalSchema"),
        "common_Shared_Asset" => schema == "BumpMapSchema",
        "common_Tint_color_colorspace" => schema != "BumpMapSchema",
        "interior_model" => schema == "PrismMetalSchema",
        _ => true,
    }
}

fn read_value(
    bytes: &[u8],
    at: &mut usize,
    carrier: Carrier,
    id: &str,
    connectable: bool,
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
        Carrier::String | Carrier::Uuid => {
            PropertyValue::String(take_lp_utf8_capped(bytes, at, 1_048_576).ok_or_else(malformed)?)
        }
        Carrier::Color => {
            if connectable || id != "common_Tint_color" {
                let marker = take::<1>(bytes, at).ok_or_else(malformed)?;
                if marker != [0] {
                    return Err(CodecError::Malformed(format!(
                        "Protein Color property {id} has invalid value marker {}",
                        marker[0]
                    )));
                }
            }
            let mut rgba = [0.0; 4];
            for value in &mut rgba {
                *value = f64::from_le_bytes(take(bytes, at).ok_or_else(malformed)?);
            }
            PropertyValue::Color(rgba)
        }
        Carrier::Reference => PropertyValue::Reference,
        Carrier::TextureUri => {
            take::<1>(bytes, at).ok_or_else(malformed)?;
            let count = usize::try_from(u32::from_le_bytes(take(bytes, at).ok_or_else(malformed)?))
                .map_err(|_| {
                    CodecError::Malformed("Protein TextureURI count exceeds usize".into())
                })?;
            if count > 1_024 {
                return Err(CodecError::Malformed(format!(
                    "Protein TextureURI property {id} has implausible path count {count}"
                )));
            }
            let mut paths = Vec::with_capacity(count);
            for _ in 0..count {
                paths.push(take_lp_utf8_capped(bytes, at, 1_048_576).ok_or_else(malformed)?);
            }
            PropertyValue::TextureUri(paths)
        }
    })
}

fn read_connections(bytes: &[u8], at: &mut usize) -> Result<Vec<String>, CodecError> {
    let Some(flag) = take::<1>(bytes, at) else {
        return Err(CodecError::Malformed(
            "Protein property connection flag is truncated".into(),
        ));
    };
    if flag == [0] {
        return Ok(Vec::new());
    }
    if flag != [1] {
        return Err(CodecError::Malformed(format!(
            "Protein property has invalid connection flag {}",
            flag[0]
        )));
    }
    let count = usize::try_from(u32::from_le_bytes(take(bytes, at).ok_or_else(|| {
        CodecError::Malformed("Protein property connection count is truncated".into())
    })?))
    .map_err(|_| CodecError::Malformed("Protein connection count exceeds usize".into()))?;
    if count > 1_024 {
        return Err(CodecError::Malformed(format!(
            "Protein property has implausible connection count {count}"
        )));
    }
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
    fn inherited_property_selection_is_schema_defined() {
        assert!(!instance_property_serializes(
            "BumpMapSchema",
            "ExchangeGUID"
        ));
        assert!(!instance_property_serializes(
            "BumpMapSchema",
            "common_Tint_color_colorspace"
        ));
        assert!(instance_property_serializes(
            "BumpMapSchema",
            "common_Shared_Asset"
        ));
        assert!(!instance_property_serializes(
            "UnifiedBitmapSchema",
            "common_Shared_Asset"
        ));
        assert!(instance_property_serializes(
            "PrismMetalSchema",
            "interior_model"
        ));
        assert!(!instance_property_serializes(
            "PrismOpaqueSchema",
            "interior_model"
        ));
    }

    #[test]
    fn color_and_connection_carriers_distinguish_their_prefixes() {
        let rgba = [0.1_f64, 0.2, 0.3, 1.0];
        let mut prefixed = vec![0];
        prefixed.extend(rgba.into_iter().flat_map(f64::to_le_bytes));
        let mut at = 0;
        assert_eq!(
            read_value(&prefixed, &mut at, Carrier::Color, "generic_diffuse", true).unwrap(),
            PropertyValue::Color(rgba)
        );
        assert_eq!(at, prefixed.len());

        let bare = rgba
            .into_iter()
            .flat_map(f64::to_le_bytes)
            .collect::<Vec<_>>();
        let mut at = 0;
        assert_eq!(
            read_value(&bare, &mut at, Carrier::Color, "common_Tint_color", false).unwrap(),
            PropertyValue::Color(rgba)
        );
        assert_eq!(at, bare.len());

        let mut connections = vec![1];
        connections.extend_from_slice(&2u32.to_le_bytes());
        push_lp(&mut connections, "first-guid");
        push_lp(&mut connections, "second-guid");
        let mut at = 0;
        assert_eq!(
            read_connections(&connections, &mut at).unwrap(),
            ["first-guid", "second-guid"]
        );
        assert_eq!(at, connections.len());
    }

    #[test]
    fn schema_driven_record_uses_inheritance_and_serialized_property_ids() {
        let protein = schema_archive(&[
            (
                "Schemas/CommonSchema.xml",
                r#"<Schema>
                    <UID val="CommonSchema"/>
                    <Color id="A_color" allowconnectedassets="true"/>
                    <Boolean id="ignored_readonly" readonly="true"/>
                    <Integer id="ignored_non_public" public="false"/>
                </Schema>"#,
            ),
            (
                "Asset/Schemas/TextureSchema.xml",
                r#"<Schema>
                    <UID val="TextureSchema"/>
                    <Base val="CommonSchema"/>
                    <PropertyAlias id="renamed_color" property="A_color"/>
                    <Distance id="B_distance"/>
                    <TextureURI id="C_uri"/>
                    <Float id="D_unit_float" unit="unitless"/>
                    <Reference id="E_reference"/>
                    <Integer id="ignored_definition" definitionIteratorData="true"/>
                    <String id="ignored_metadata" metadata="true"/>
                </Schema>"#,
            ),
        ]);
        let mut logical = RECORD_MARKER.to_vec();
        for value in ["TextureSchema", "asset-guid", "Texture", ""] {
            push_lp(&mut logical, value);
        }
        logical.push(0);
        for value in [0.1_f64, 0.2, 0.3, 1.0] {
            logical.extend_from_slice(&value.to_le_bytes());
        }
        push_connections(&mut logical, &["first-guid", "second-guid"]);
        logical.extend_from_slice(&0x2016_u32.to_le_bytes());
        logical.extend_from_slice(&2.5_f64.to_le_bytes());
        logical.push(0);
        logical.extend_from_slice(&2u32.to_le_bytes());
        push_lp(&mut logical, "cloud/resource/one");
        push_lp(&mut logical, "cloud/resource/two");
        logical.extend_from_slice(&0x200e_u32.to_le_bytes());
        logical.extend_from_slice(&4.5_f64.to_le_bytes());
        push_connections(&mut logical, &["reference-guid"]);

        let mut instance_stream = RECORD_MARKER.to_vec();
        for value in ["TextureSchema", "truncated-guid", "Truncated", ""] {
            push_lp(&mut instance_stream, value);
        }
        instance_stream.extend_from_slice(&logical);
        let records = decode(&protein, &instance_stream).expect("schema record decodes");
        assert_eq!(records.len(), 1);
        let properties = &records[0].properties;
        assert_eq!(
            properties["A_color"],
            DecodedProperty {
                value: PropertyValue::Color([0.1, 0.2, 0.3, 1.0]),
                connections: vec!["first-guid".into(), "second-guid".into()],
            }
        );
        assert_eq!(
            properties["B_distance"].value,
            PropertyValue::Distance {
                unit: 0x2016,
                value: 2.5,
            }
        );
        assert_eq!(
            properties["C_uri"].value,
            PropertyValue::TextureUri(vec![
                "cloud/resource/one".into(),
                "cloud/resource/two".into(),
            ])
        );
        assert_eq!(properties["D_unit_float"].value, PropertyValue::Float(4.5));
        assert_eq!(
            properties["E_reference"].connections,
            vec!["reference-guid"]
        );
        assert!(!properties.contains_key("renamed_color"));
        assert!(!properties.contains_key("ignored_readonly"));
        assert!(!properties.contains_key("ignored_non_public"));
        assert!(!properties.contains_key("ignored_definition"));
        assert!(!properties.contains_key("ignored_metadata"));
    }

    fn schema_archive(entries: &[(&str, &str)]) -> Vec<u8> {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
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
        bytes.push(1);
        bytes.extend_from_slice(&(values.len() as u32).to_le_bytes());
        for value in values {
            push_lp(bytes, value);
        }
    }
}
