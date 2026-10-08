// SPDX-License-Identifier: Apache-2.0
//! Schema-driven decoding of Protein `InstanceProperties` records.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

use admission::ProteinAdmission;

/// Typed admission shared by decode and writers.
pub mod admission;

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
pub(crate) const MAX_SCHEMA_BYTES: u64 = 128 * 1024 * 1024;
const MAX_RECOVERY_VALUES: u64 = 1_024;

fn take_lp_utf8_capped<'bytes, A: ProteinAdmission>(
    admission: A,
    bytes: &'bytes [u8],
    at: &mut usize,
    max: usize,
) -> Result<Option<&'bytes str>, CodecError>
where
    CodecError: From<A::Error>,
{
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
    let Some(value_bytes) = bytes.get(*at..end) else {
        return Ok(None);
    };
    let Ok(value) = admission.validate_utf8(value_bytes, "Protein decoded string UTF-8")? else {
        return Ok(None);
    };
    *at = end;
    Ok(Some(value))
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

#[derive(Clone, Copy)]
enum SchemaCarrier {
    Reference,
    TextureUri,
    Scalar(ValueCarrier),
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
///
/// The catalog names and maps live under the admission storage owner, which
/// is released when the catalog is dropped.
pub struct SchemaCatalog<A: ProteinAdmission> {
    schemas: HashMap<String, Schema>,
    properties: HashMap<String, BTreeMap<String, Property>>,
    storage: A::Scope,
}

impl<A: ProteinAdmission> SchemaCatalog<A>
where
    CodecError: From<A::Error>,
{
    /// Parse the schemas in one nested Protein archive through the admission.
    /// Returns `None` when the archive has no schema entries.
    pub fn load(admission: A, protein: A::Archive<'_>) -> Result<Option<Self>, CodecError> {
        let mut catalog = Self::empty(admission)?;
        let Self {
            schemas, storage, ..
        } = &mut catalog;
        admission.read_schemas(protein, |name, xml| {
            parse_schema_document(admission, storage, name, xml, schemas)
        })?;
        if catalog.schemas.is_empty() {
            return Ok(None);
        }
        Ok(Some(catalog))
    }

    fn empty(admission: A) -> Result<Self, CodecError> {
        Ok(Self {
            schemas: HashMap::new(),
            properties: HashMap::new(),
            storage: admission.scope("Protein schema catalog")?,
        })
    }

    fn properties_for(
        &mut self,
        admission: A,
        name: &str,
    ) -> Result<&BTreeMap<String, Property>, CodecError> {
        resolve_inheritance(
            admission,
            &mut self.storage,
            &self.schemas,
            &mut self.properties,
            name,
        )?;
        admission
            .get_hash_map(&self.properties, name, "Protein resolved schema lookup")?
            .ok_or_else(|| {
                admission.format_text(format_args!(
                    "Protein instance references absent schema {name}"
                ), "Protein rejected record detail").map(CodecError::Malformed).unwrap_or_else(Into::into)
            })
    }
}

/// Resolves the inherited property closure of `name` and of every base it
/// names that is not yet resolved, base first.
fn resolve_inheritance<'s, A: ProteinAdmission>(
    admission: A,
    storage: &mut A::Scope,
    schemas: &'s HashMap<String, Schema>,
    resolved: &mut HashMap<String, BTreeMap<String, Property>>,
    name: &'s str,
) -> Result<(), CodecError>
where
    CodecError: From<A::Error>,
{
    let mut path_storage = admission.scope("Protein schema inheritance path")?;
    let mut active_storage = admission.scope("Protein schema inheritance active set")?;
    let mut guard_storage = admission.scope("Protein schema inheritance guards")?;
    let mut path: Vec<(&'s str, &'s Schema)> = Vec::new();
    let mut active = BTreeSet::new();
    let mut depth_guards: Vec<A::Depth> = Vec::new();
    let mut current = name;
    loop {
        admission.work(1, "Protein schema inheritance traversal")?;
        if admission.contains_key_hash_map(resolved, current, "Protein resolved schema lookup")? {
            break;
        }
        if admission.contains_name(&active, current, "Protein inheritance cycle lookup")? {
            return Err(admission.format_text(format_args!(
                "Protein schema inheritance contains a cycle at {current}"
            ), "Protein rejected record detail").map(CodecError::Malformed).unwrap_or_else(Into::into));
        }
        let schema = admission
            .get_hash_map(schemas, current, "Protein inherited schema lookup")?
            .ok_or_else(|| {
                admission.format_text(format_args!(
                    "Protein instance references absent schema {current}"
                ), "Protein rejected record detail").map(CodecError::Malformed).unwrap_or_else(Into::into)
            })?;
        let depth = admission.enter_nested("Protein schema inheritance")?;
        admission.scoped(&mut guard_storage, || {
            Ok(admission.push(
                &mut depth_guards,
                depth,
                "Protein schema inheritance guards",
            )?)
        })?;
        admission.scoped(&mut active_storage, || {
            Ok(admission.insert_name(
                &mut active,
                current,
                "Protein schema inheritance active set",
            )?)
        })?;
        admission.scoped(&mut path_storage, || {
            Ok(admission.push(
                &mut path,
                (current, schema),
                "Protein schema inheritance path",
            )?)
        })?;
        let Some(base) = schema.base.as_deref() else {
            break;
        };
        current = base;
    }
    loop {
        admission.work(1, "Protein schema inheritance closure traversal")?;
        let Some((current, schema)) = path.pop() else {
            break;
        };
        let inherited = match schema.base.as_deref() {
            Some(base) => {
                admission.get_hash_map(resolved, base, "Protein inherited closure lookup")?
            }
            None => None,
        };
        let properties = admission.scoped(storage, || {
            let mut properties = BTreeMap::new();
            if let Some(inherited) = inherited {
                for (id, property) in
                    admission.traverse(inherited, "Protein inherited property closure")?
                {
                    let id = admission.copy_text(id, "Protein inherited property names")?;
                    admission.insert_btree_map(
                        &mut properties,
                        id,
                        property.clone(),
                        "Protein inherited property closure",
                    )?;
                }
            }
            for (id, property) in
                admission.traverse(&schema.properties, "Protein inherited property closure")?
            {
                if let Some(value) = admission.get_mut_btree_map(
                    &mut properties,
                    id,
                    "Protein local property override",
                )? {
                    *value = property.clone();
                } else {
                    let id = admission.copy_text(id, "Protein inherited property names")?;
                    admission.insert_btree_map(
                        &mut properties,
                        id,
                        property.clone(),
                        "Protein inherited property closure",
                    )?;
                }
            }
            Ok::<_, CodecError>(properties)
        })?;
        admission.scoped(storage, || {
            let name = admission.copy_text(current, "Protein inherited property names")?;
            Ok(admission.insert_hash_map(resolved, name, properties, "Protein resolved schema")?)
        })?;
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
        None => SchemaCatalog::empty(ctx)?,
    };
    let frames = framing::record_frames_admitted(ctx, instance.window())?;
    decode_frames_admitted(ctx, &mut catalog, frames.frames())
}

/// Decode frames admitted by the caller against one parsed schema catalog.
pub fn decode_frames_admitted<A: ProteinAdmission>(
    admission: A,
    catalog: &mut SchemaCatalog<A>,
    frames: &[framing::RecordFrame],
) -> Result<DecodeOutcome, CodecError>
where
    CodecError: From<A::Error>,
{
    let mut outcome = DecodeOutcome::default();
    for (ordinal, frame) in admission
        .traverse(frames, "Protein record outcome traversal")?
        .enumerate()
    {
        let ordinal = cadmpeg_core::decode::u64_from_index(ordinal);
        match decode_record(
            admission,
            frame.bytes(),
            catalog,
            ordinal,
            frame.logical_offset(),
        ) {
            Ok(Some(record)) => {
                admission.push(&mut outcome.records, record, "Protein record outcome")?;
            }
            Ok(None) => {
                const DETAIL: &str = "Protein instance record header is malformed";
                let detail = admission.copy_text(DETAIL, "Protein rejected record detail")?;
                admission.push(
                    &mut outcome.rejected,
                    RejectedRecord { ordinal, detail },
                    "Protein record outcome",
                )?;
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                let detail = admission
                    .format_text(format_args!("{error}"), "Protein rejected record detail")?;
                admission.push(
                    &mut outcome.rejected,
                    RejectedRecord { ordinal, detail },
                    "Protein record outcome",
                )?;
            }
        }
    }
    Ok(outcome)
}

fn parse_schema_document<A: ProteinAdmission>(
    admission: A,
    storage: &mut A::Scope,
    name: &str,
    bytes: &[u8],
    schemas: &mut HashMap<String, Schema>,
) -> Result<(), CodecError>
where
    CodecError: From<A::Error>,
{
    let xml = admission
        .validate_utf8(bytes, "validate Protein XML UTF-8")?
        .map_err(|error| {
            admission.format_text(format_args!("Protein schema {name} is not UTF-8: {error}"), "Protein malformed detail").map(CodecError::Malformed).unwrap_or_else(Into::into)
        })?;
    admission
        .with_xml(xml, "Protein schema XML tree", |document| {
            let root = admission
                .xml_root(document, "Protein schema root search")?
                .ok_or_else(|| {
                    admission.format_text(format_args!("Protein schema {name} has no root"), "Protein malformed detail").map(CodecError::Malformed).unwrap_or_else(Into::into)
                })?;
            let mut uid_node = None;
            let mut children = root.children();
            // The tag comparison is against a fixed three-byte name.
            while let Some(node) =
                admission.next(&mut children, "Protein schema UID node search")?
            {
                if node.has_tag_name("UID") {
                    uid_node = Some(node);
                    break;
                }
            }
            let uid_node = uid_node.ok_or_else(|| {
                admission.format_text(format_args!("Protein schema {name} has no UID"), "Protein malformed detail").map(CodecError::Malformed).unwrap_or_else(Into::into)
            })?;
            let uid = admission
                .xml_attribute(uid_node, "val", "Protein schema UID search")?
                .ok_or_else(|| {
                    admission.format_text(format_args!("Protein schema {name} has no UID"), "Protein malformed detail").map(CodecError::Malformed).unwrap_or_else(Into::into)
                })?;
            let mut schema = Schema::default();
            let mut base = None;
            let mut children = root.children();
            while let Some(node) = admission.next(&mut children, "Protein schema child scan")? {
                if !node.is_element() {
                    continue;
                }
                if node.has_tag_name("Base") {
                    if let Some(value) =
                        admission.xml_attribute(node, "val", "Protein schema base search")?
                    {
                        base = Some(value);
                    }
                    continue;
                }
                if node.has_tag_name("PropertyAlias") {
                    continue;
                }
                let Some(property) = schema_property(admission, node)? else {
                    continue;
                };
                let Some(id) = admission.xml_attribute(node, "id", "Protein property id search")?
                else {
                    continue;
                };
                let replaced = admission.scoped(storage, || {
                    let key = admission.copy_text(id, "Protein schema property name")?;
                    Ok(admission.insert_btree_map(
                        &mut schema.properties,
                        key,
                        property,
                        "Protein schema property",
                    )?)
                })?;
                if replaced.is_some() {
                    return Err(admission.format_text(format_args!(
                        "Protein schema {uid} declares property {id} more than once"
                    ), "Protein malformed detail").map(CodecError::Malformed).unwrap_or_else(Into::into));
                }
            }
            schema.base = base
                .map(|value| admission.scoped(storage, || {
                    Ok(admission.copy_text(value, "Protein schema base name")?)
                }))
                .transpose()?;
            let replaced = admission.scoped(storage, || {
                let key = admission.copy_text(uid, "Protein schema UID")?;
                Ok(admission.insert_hash_map(schemas, key, schema, "Protein parsed schema")?)
            })?;
            if replaced.is_some() {
                return Err(admission.format_text(format_args!(
                    "Protein archive defines schema {uid} more than once"
                ), "Protein malformed detail").map(CodecError::Malformed).unwrap_or_else(Into::into));
            }
            Ok(())
        })
        .map_err(|error| {
            let CodecError::Malformed(error) = error else {
                return error;
            };
            admission.format_text(format_args!(
                "Protein schema {name} is malformed XML: {error}"
            ), "Protein malformed detail").map(CodecError::Malformed).unwrap_or_else(Into::into)
        })?
}

fn schema_property<A: ProteinAdmission>(
    admission: A,
    node: roxmltree::Node<'_, '_>,
) -> Result<Option<Property>, CodecError>
where
    CodecError: From<A::Error>,
{
    admission.work(0, "Protein schema property")?;
    let carrier = match node.tag_name().name() {
        "Reference" => SchemaCarrier::Reference,
        "TextureURI" => SchemaCarrier::TextureUri,
        "Boolean" => SchemaCarrier::Scalar(ValueCarrier::Boolean),
        "Integer" | "Choice" => SchemaCarrier::Scalar(ValueCarrier::Integer),
        "Float" => SchemaCarrier::Scalar(ValueCarrier::Float),
        "Distance" => SchemaCarrier::Scalar(ValueCarrier::Distance),
        "String" | "Uuid" | "URL" => SchemaCarrier::Scalar(ValueCarrier::String),
        "Color" => SchemaCarrier::Scalar(ValueCarrier::Color),
        _ => return Ok(None),
    };
    if admission.xml_attribute(node, "readonly", "Protein readonly attribute search")?
        == Some("true")
        || admission.xml_attribute(
            node,
            "definitionIteratorData",
            "Protein definition attribute search",
        )? == Some("true")
    {
        return Ok(None);
    }
    let multiple = || {
        admission.xml_attribute(node, "allowmultiplevalues", "Protein multiple-values attribute search")
            .map(|value| value == Some("true"))
    };
    let connectable = || {
        admission.xml_attribute(node, "allowconnectedassets", "Protein connected-assets attribute search")
            .map(|value| value.is_some())
    };
    Ok(Some(match carrier {
        SchemaCarrier::Reference => Property::Reference { multiple: multiple()? },
        SchemaCarrier::TextureUri => Property::Value {
            layout: ValueLayout::TextureUri,
            connectable: connectable()?,
        },
        SchemaCarrier::Scalar(mut carrier) => {
            let multiple = multiple()?;
            let connectable = connectable()?;
            if matches!(carrier, ValueCarrier::Float)
                && admission.xml_attribute(node, "unit", "Protein unit attribute search")?.is_some()
            {
                carrier = ValueCarrier::UnitFloat;
            }
            Property::Value {
                layout: if multiple { ValueLayout::Multiple(carrier) } else { ValueLayout::Single(carrier) },
                connectable,
            }
        },
    }))
}

fn decode_record<A: ProteinAdmission>(
    admission: A,
    record: &[u8],
    catalog: &mut SchemaCatalog<A>,
    ordinal: u64,
    logical_offset: usize,
) -> Result<Option<DecodedRecord>, CodecError>
where
    CodecError: From<A::Error>,
{
    if !record.starts_with(RECORD_MARKER) {
        return Ok(None);
    }
    let mut at = RECORD_MARKER.len();
    let Some(schema) = take_lp_utf8_capped(admission, record, &mut at, 1_048_576)? else {
        return Ok(None);
    };
    let Some(guid) = take_lp_utf8_capped(admission, record, &mut at, 1_048_576)? else {
        return Ok(None);
    };
    let Some(base) = take_lp_utf8_capped(admission, record, &mut at, 1_048_576)? else {
        return Ok(None);
    };
    // The fourth header string is `AssetLibID`, the first member of
    // `CommonSchema` in serialization order. It is carried in the record header
    // rather than in the value block, so `instance_property_serializes` drops
    // the member there.
    let Some(asset_lib_id) = take_lp_utf8_capped(admission, record, &mut at, 1_048_576)? else {
        return Ok(None);
    };
    let properties = catalog.properties_for(admission, schema)?;
    let mut values = BTreeMap::new();
    for (id, property) in admission.traverse(properties, "Protein decoded properties")? {
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
                admission.format_text(format_args!(
                    "Protein {schema} instance {guid} property {id} offset overflows usize"
                ), "Protein rejected record detail").map(CodecError::Malformed).unwrap_or_else(Into::into)
            })?
        } else {
            property_at
        };
        let value_error = |error: CodecError, at: usize| {
            if matches!(&error, CodecError::ResourceLimit(_)) {
                return error;
            }
            admission.format_text(format_args!(
                "Protein {schema} instance {guid} property {id} at {property_at}..{at}/{}: {error}",
                record.len()
            ), "Protein rejected record detail").map(CodecError::Malformed).unwrap_or_else(Into::into)
        };
        let connection_error = |error: CodecError, at: usize| {
            if matches!(&error, CodecError::ResourceLimit(_)) {
                return error;
            }
            admission.format_text(format_args!(
                "Protein {schema} instance {guid} property {id} connection at {at}/{}: {error}",
                record.len()
            ), "Protein rejected record detail").map(CodecError::Malformed).unwrap_or_else(Into::into)
        };
        let content = match property {
            Property::Reference { multiple } => {
                let count = (*multiple)
                    .then(|| read_count(admission, record, &mut at, id))
                    .transpose()
                    .map_err(|error| value_error(error, at))?;
                let targets = read_connections(admission, record, &mut at)
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
                let value = read_property(admission, record, &mut at, *layout, id)
                    .map_err(|error| value_error(error, at))?;
                let connections = (*connectable)
                    .then(|| read_connections(admission, record, &mut at))
                    .transpose()
                    .map_err(|error| connection_error(error, at))?
                    .unwrap_or_default();
                PropertyContent::Value { value, connections }
            }
        };
        admission.insert_btree_map(
            &mut values,
            admission.copy_text(id, "Protein decoded property name")?,
            DecodedProperty {
                value_offset,
                content,
            },
            "Protein decoded property",
        )?;
    }
    if at != record.len() {
        return Err(admission.format_text(format_args!(
            "Protein {schema} instance {guid} consumed {at} of {} record bytes",
            record.len()
        ), "Protein rejected record detail").map(CodecError::Malformed).unwrap_or_else(Into::into));
    }
    Ok(Some(DecodedRecord {
        ordinal,
        logical_offset,
        schema: admission.copy_text(schema, "Protein decoded string")?,
        guid: admission.copy_text(guid, "Protein decoded string")?,
        base: admission.copy_text(base, "Protein decoded string")?,
        asset_lib_id: admission.copy_text(asset_lib_id, "Protein decoded string")?,
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

fn read_property<A: ProteinAdmission>(
    admission: A,
    bytes: &[u8],
    at: &mut usize,
    layout: ValueLayout,
    id: &str,
) -> Result<PropertyValue, CodecError>
where
    CodecError: From<A::Error>,
{
    match layout {
        ValueLayout::Single(carrier) => read_value(admission, bytes, at, carrier, id),
        // TextureURI owns its kind byte and optional count; the schema's
        // multiple-value declaration does not add another count prefix.
        ValueLayout::TextureUri => read_texture_uri(admission, bytes, at, id),
        ValueLayout::Multiple(carrier) => {
            let count = read_count(admission, bytes, at, id)?;
            let values =
                admission.collect_indexed(count, "Protein multiple property members", |_| {
                    read_value(admission, bytes, at, carrier, id)
                })?;
            // `read_value` yields the one scalar variant its carrier selects,
            // so the members already share one scalar carrier.
            Ok(PropertyValue::Multiple(
                RepeatedValues::of_one_scalar_carrier(values),
            ))
        }
    }
}

/// A `TextureURI` value: a kind byte, then either a counted list of paths
/// (kind 0, used for cloud resource references) or a single path (kind 1).
fn read_texture_uri<A: ProteinAdmission>(
    admission: A,
    bytes: &[u8],
    at: &mut usize,
    id: &str,
) -> Result<PropertyValue, CodecError>
where
    CodecError: From<A::Error>,
{
    let malformed = || admission.format_text(format_args!("Protein property {id} is truncated"), "Protein rejected record detail").map(CodecError::Malformed).unwrap_or_else(Into::into);
    let kind = take::<1>(bytes, at).ok_or_else(malformed)?[0];
    if kind == 1 {
        return Ok(PropertyValue::TextureUri(admission.collect_indexed(
            1,
            "Protein texture URI paths",
            |_| {
                let value = take_lp_utf8_capped(admission, bytes, at, 1_048_576)?
                    .ok_or_else(malformed)?;
                Ok(admission.copy_text(value, "Protein decoded string")?)
            },
        )?));
    }
    if kind != 0 {
        return Err(admission.format_text(format_args!(
            "Protein TextureURI property {id} has invalid kind {kind}"
        ), "Protein rejected record detail").map(CodecError::Malformed).unwrap_or_else(Into::into));
    }
    let count = read_count(admission, bytes, at, id)?;
    let paths = admission.collect_indexed(count, "Protein texture URI paths", |_| {
        let value = take_lp_utf8_capped(admission, bytes, at, 1_048_576)?
            .ok_or_else(malformed)?;
        Ok(admission.copy_text(value, "Protein decoded string")?)
    })?;
    Ok(PropertyValue::TextureUri(paths))
}

fn read_count<A: ProteinAdmission>(
    admission: A,
    bytes: &[u8],
    at: &mut usize,
    id: &str,
) -> Result<usize, CodecError>
where
    CodecError: From<A::Error>,
{
    let count = usize::try_from(read_u32_le(bytes, at).ok_or_else(|| {
        admission.format_text(format_args!("Protein property {id} is truncated"), "Protein rejected record detail").map(CodecError::Malformed).unwrap_or_else(Into::into)
    })?)
    .map_err(|_| CodecError::Malformed("Protein value count exceeds usize".into()))?;
    let population = cadmpeg_core::decode::u64_from_index(count);
    if population > MAX_RECOVERY_VALUES {
        return Err(admission.format_ceiling(
            "Protein counted value recovery",
            MAX_RECOVERY_VALUES,
            population,
        ));
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

fn read_value<A: ProteinAdmission>(
    admission: A,
    bytes: &[u8],
    at: &mut usize,
    carrier: ValueCarrier,
    id: &str,
) -> Result<PropertyValue, CodecError>
where
    CodecError: From<A::Error>,
{
    let malformed = || admission.format_text(format_args!("Protein property {id} is truncated"), "Protein rejected record detail").map(CodecError::Malformed).unwrap_or_else(Into::into);
    Ok(match carrier {
        ValueCarrier::Boolean => {
            PropertyValue::Boolean(take::<1>(bytes, at).ok_or_else(malformed)?[0] != 0)
        }
        ValueCarrier::Integer => {
            PropertyValue::Integer(read_u32_le(bytes, at).ok_or_else(malformed)?)
        }
        ValueCarrier::Float => PropertyValue::Float(finite_value(
            admission,
            read_f64_le(bytes, at).ok_or_else(malformed)?,
            id,
        )?),
        ValueCarrier::UnitFloat => {
            take::<4>(bytes, at).ok_or_else(malformed)?;
            PropertyValue::Float(finite_value(
            admission,
                read_f64_le(bytes, at).ok_or_else(malformed)?,
                id,
            )?)
        }
        ValueCarrier::Distance => PropertyValue::Distance {
            unit: read_u32_le(bytes, at).ok_or_else(malformed)?,
            value: finite_value(admission, read_f64_le(bytes, at).ok_or_else(malformed)?, id)?,
        },
        ValueCarrier::String => {
            let value = take_lp_utf8_capped(admission, bytes, at, 1_048_576)?
                .ok_or_else(malformed)?;
            PropertyValue::String(admission.copy_text(value, "Protein decoded string")?)
        }
        ValueCarrier::Color => {
            let mut rgba = [FiniteReal::ZERO; 4];
            for value in &mut rgba {
                *value = finite_value(admission, read_f64_le(bytes, at).ok_or_else(malformed)?, id)?;
            }
            PropertyValue::Color(rgba)
        }
    })
}

fn finite_value<A: ProteinAdmission>(admission: A, value: f64, id: &str) -> Result<FiniteReal, CodecError>
where
    CodecError: From<A::Error>,
{
    FiniteReal::new(value)
        .ok_or_else(|| admission.format_text(format_args!("Protein property {id} is not finite"), "Protein rejected record detail").map(CodecError::Malformed).unwrap_or_else(Into::into))
}

/// The connection block that follows every connectable member and every
/// `Reference`: a presence byte, then a kind byte, a `u32` count, and that many
/// length-prefixed connected-asset GUIDs.
fn read_connections<A: ProteinAdmission>(
    admission: A,
    bytes: &[u8],
    at: &mut usize,
) -> Result<Vec<String>, CodecError>
where
    CodecError: From<A::Error>,
{
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
    let count = read_count(admission, bytes, at, "connection")?;
    admission.collect_indexed(count, "Protein connected asset GUIDs", |_| {
        let value = take_lp_utf8_capped(admission, bytes, at, 1_048_576)?.ok_or_else(|| {
            CodecError::Malformed("Protein property connection GUID is truncated".into())
        })?;
        Ok(admission.copy_text(value, "Protein decoded string")?)
    })
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

    use std::collections::{BTreeMap, HashMap};

    use cadmpeg_core::decode::{
        DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ScopedReservation,
    };
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

    fn assert_schema_diagnostic_admitted(name: &str, xml: &[u8], expected: &str, operation: &str) {
        with_service_context(xml, |ctx| {
            let error = super::parse_schema_document(
                ctx, &mut scratch(ctx), name, xml, &mut HashMap::new(),
            ).expect_err("invalid schema");
            let CodecError::Malformed(detail) = error else {
                panic!("service profile preserves structural error");
            };
            assert!(detail.contains(expected), "{detail}");
        });
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(xml, &arena, &policy).expect("input");
        let error = super::parse_schema_document(
            &ctx, &mut scratch(&ctx), name, xml, &mut HashMap::new(),
        ).expect_err("diagnostic storage requires admission");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected diagnostic refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(limit.operation, operation);
        assert_eq!(ctx.resource_refusal(), Some(limit.clone()));
        assert!(matches!(ctx.charge_work(0, "after diagnostic refusal"),
            Err(CodecError::ResourceLimit(original)) if original == limit));
    }

    #[test]
    fn invalid_schema_utf8_diagnostic_admits_entry_name() {
        let name = "schema".repeat(256);
        assert_schema_diagnostic_admitted(&name, &[0xff], &format!("Protein schema {name} is not UTF-8:"), "Protein malformed detail");
    }

    #[test]
    fn missing_schema_uid_diagnostic_admits_entry_name() {
        let name = "schema".repeat(256);
        assert_schema_diagnostic_admitted(&name, b"<Schema/>", &format!("Protein schema {name} has no UID"), "Protein malformed detail");
    }

    #[test]
    fn missing_schema_uid_attribute_diagnostic_admits_entry_name() {
        let name = "schema".repeat(256);
        assert_schema_diagnostic_admitted(&name, b"<Schema><UID/></Schema>", &format!("Protein schema {name} has no UID"), "Protein malformed detail");
    }

    #[test]
    fn malformed_schema_xml_diagnostic_admits_entry_name() {
        let name = "schema".repeat(256);
        assert_schema_diagnostic_admitted(&name, b"<Schema>", &format!("Protein schema {name} is malformed XML:"), "Protein schema XML tree");
    }

    #[test]
    fn duplicate_property_diagnostic_admits_schema_and_property_ids() {
        let uid = "uid".repeat(256);
        let id = "id".repeat(256);
        let xml = format!("<Schema><UID val=\"{uid}\"/><String id=\"{id}\"/><String id=\"{id}\"/></Schema>");
        assert_schema_diagnostic_admitted("schema", xml.as_bytes(),
            &format!("Protein schema {uid} declares property {id} more than once"), "Protein malformed detail");
    }

    #[test]
    fn duplicate_schema_diagnostic_admits_schema_id() {
        let uid = "uid".repeat(256);
        let xml = format!("<Schema><UID val=\"{uid}\"/></Schema>");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy).expect("input");
        let mut schemas = HashMap::from([(uid.clone(), super::Schema::default())]);
        let error = super::parse_schema_document(&ctx, &mut scratch(&ctx), "schema", xml.as_bytes(), &mut schemas)
            .expect_err("duplicate schema diagnostic must be admitted");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "Protein malformed detail"));
        with_service_context(xml.as_bytes(), |ctx| {
            let mut schemas = HashMap::from([(uid.clone(), super::Schema::default())]);
            let error = super::parse_schema_document(ctx, &mut scratch(ctx), "schema", xml.as_bytes(), &mut schemas)
                .expect_err("duplicate schema");
            assert!(matches!(error, CodecError::Malformed(detail)
                if detail == format!("Protein archive defines schema {uid} more than once")));
        });
    }

    #[test]
    fn inheritance_cycle_diagnostic_admits_schema_id() {
        let name = "schema".repeat(256);
        let schemas = HashMap::from([(name.clone(), super::Schema {
            base: Some(name.clone()), ..super::Schema::default()
        })]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("input");
        let error = super::resolve_inheritance(&ctx, &mut scratch(&ctx), &schemas, &mut HashMap::new(), &name)
            .expect_err("cycle diagnostic must be admitted");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "Protein rejected record detail"));
        with_service_context(&[], |ctx| {
            let error = super::resolve_inheritance(ctx, &mut scratch(ctx), &schemas, &mut HashMap::new(), &name)
                .expect_err("cycle");
            assert!(matches!(error, CodecError::Malformed(detail)
                if detail == format!("Protein schema inheritance contains a cycle at {name}")));
        });
    }

    #[test]
    fn absent_schema_diagnostic_admits_schema_id() {
        let name = "schema".repeat(256);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("input");
        let error = super::resolve_inheritance(&ctx, &mut scratch(&ctx), &HashMap::new(), &mut HashMap::new(), &name)
            .expect_err("missing schema diagnostic must be admitted");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "Protein rejected record detail"));
        with_service_context(&[], |ctx| {
            let error = super::resolve_inheritance(ctx, &mut scratch(ctx), &HashMap::new(), &mut HashMap::new(), &name)
                .expect_err("absent schema");
            assert!(matches!(error, CodecError::Malformed(detail)
                if detail == format!("Protein instance references absent schema {name}")));
        });
    }

    #[test]
    fn truncated_property_diagnostic_admits_property_id() {
        let id = "property".repeat(256);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("input");
        let error = read_value(&ctx, &[], &mut 0, ValueCarrier::Integer, &id)
            .expect_err("truncation diagnostic must be admitted");
        let CodecError::ResourceLimit(limit) = error else { panic!("diagnostic refusal"); };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "Protein rejected record detail");
        assert_eq!(ctx.resource_refusal(), Some(limit));
        with_service_context(&[], |ctx| {
            let error = read_value(ctx, &[], &mut 0, ValueCarrier::Integer, &id).expect_err("truncated");
            assert!(matches!(error, CodecError::Malformed(detail)
                if detail == format!("Protein property {id} is truncated")));
        });
    }

    #[test]
    fn nonfinite_property_diagnostic_admits_property_id() {
        let id = "property".repeat(256);
        let bytes = f64::INFINITY.to_le_bytes();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input");
        let error = read_value(&ctx, &bytes, &mut 0, ValueCarrier::Float, &id)
            .expect_err("nonfinite diagnostic must be admitted");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "Protein rejected record detail"));
        with_service_context(&bytes, |ctx| {
            let error = read_value(ctx, &bytes, &mut 0, ValueCarrier::Float, &id).expect_err("nonfinite");
            assert!(matches!(error, CodecError::Malformed(detail)
                if detail == format!("Protein property {id} is not finite")));
        });
    }

    #[test]
    fn invalid_texture_kind_diagnostic_admits_property_id() {
        let id = "property".repeat(256);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[2], &arena, &policy).expect("input");
        let error = read_texture_uri(&ctx, &[2], &mut 0, &id)
            .expect_err("texture kind diagnostic must be admitted");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "Protein rejected record detail"));
        with_service_context(&[2], |ctx| {
            let error = read_texture_uri(ctx, &[2], &mut 0, &id).expect_err("invalid texture kind");
            assert!(matches!(error, CodecError::Malformed(detail)
                if detail == format!("Protein TextureURI property {id} has invalid kind 2")));
        });
    }

    #[test]
    fn standard_admission_matches_decode_context_records() {
        let protein = schema_archive(&[
            (
                "Schemas/RootSchema.xml",
                r#"<Schema>
                    <UID val="Root"/>
                    <String id="AssetLibID"/>
                    <Color id="a_color" allowconnectedassets="single"/>
                    <Boolean id="c_comment"/>
                </Schema>"#,
            ),
            (
                "Asset/Schemas/ChildSchema.xml",
                r#"<Schema>
                    <UID val="Child"/>
                    <Base val="Root"/>
                    <String id="c_comment"/>
                    <TextureURI id="d_paths" allowmultiplevalues="true"/>
                    <Reference id="e_targets" allowmultiplevalues="true"/>
                    <Integer id="f_values" allowmultiplevalues="true"/>
                </Schema>"#,
            ),
        ]);
        let mut records = Vec::new();
        for guid in ["first-guid", "second-guid"] {
            let mut record = Vec::new();
            for value in ["Child", guid, "base", "library"] {
                push_lp(&mut record, value);
            }
            for value in [0.1_f64, 0.2, 0.3, 1.0] {
                record.extend_from_slice(&value.to_le_bytes());
            }
            push_connections(&mut record, &["color-guid"]);
            push_lp(&mut record, &"comment".repeat(50));
            record.push(0);
            record.extend_from_slice(&2_u32.to_le_bytes());
            push_lp(&mut record, "cloud/first");
            push_lp(&mut record, "cloud/second");
            record.extend_from_slice(&2_u32.to_le_bytes());
            push_connections(&mut record, &["first-target", "second-target"]);
            record.extend_from_slice(&2_u32.to_le_bytes());
            record.extend_from_slice(&3_u32.to_le_bytes());
            record.extend_from_slice(&7_u32.to_le_bytes());
            records.push(record);
        }
        let mut absent_schema = Vec::new();
        for value in ["Absent", "missing-guid", "base", "library"] {
            push_lp(&mut absent_schema, value);
        }
        let instance = paged_stream(&[&records[0], b"bad header", &absent_schema, &records[1]]);
        let expected = decode_fixture(&protein, &instance).expect("context decode");
        assert_eq!(expected.records.len(), 2);
        assert_eq!(expected.rejected.len(), 2);
        let admission = super::admission::StandardAdmission;
        let mut catalog = super::SchemaCatalog::load(admission, protein.as_slice())
            .expect("standard schema load")
            .expect("schema entries");
        let frames =
            framing::record_frames_admitted(admission, &instance).expect("standard page framing");
        let actual = super::decode_frames_admitted(admission, &mut catalog, frames.frames())
            .expect("standard decode");
        assert_eq!(actual, expected);
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

    /// Frames a paged stream under a service context and keeps copies of the
    /// frames past that context.
    fn frames_of(stream: &[u8]) -> Result<Vec<framing::RecordFrame>, CodecError> {
        with_service_context(stream, |ctx| {
            Ok(framing::record_frames_admitted(ctx, stream)?
                .frames()
                .to_vec())
        })
    }

    /// Empty scoped storage for a schema parse or inheritance resolution.
    fn scratch<'ctx>(ctx: &'ctx DecodeContext<'_>) -> ScopedReservation<'ctx> {
        ctx.reserve_scoped(0, "test schema catalog")
            .expect("empty reservation")
    }

    /// A catalog from parsed schemas and already resolved property closures.
    fn catalog_of<'ctx, 'input>(
        ctx: &'ctx DecodeContext<'input>,
        schemas: HashMap<String, super::Schema>,
        properties: HashMap<String, BTreeMap<String, super::Property>>,
    ) -> super::SchemaCatalog<&'ctx DecodeContext<'input>> {
        super::SchemaCatalog {
            schemas,
            properties,
            storage: scratch(ctx),
        }
    }

    #[test]
    fn schema_attribute_preserves_first_local_name_match() {
        let document =
            roxmltree::Document::parse(r#"<UID xmlns:p="urn:test" p:val="first" val="second"/>"#)
                .unwrap();
        with_service_context(&[], |ctx| {
            assert_eq!(
                ctx.xml_attribute(document.root_element(), "val", "attribute test")
                    .unwrap(),
                Some("first")
            );
            assert_eq!(
                ctx.xml_attribute(document.root_element(), "missing", "attribute test")
                    .unwrap(),
                None
            );
        });
    }

    #[test]
    fn schema_attribute_propagates_work_refusal() {
        let document = roxmltree::Document::parse(r#"<UID val="value"/>"#).unwrap();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = ctx
            .xml_attribute(document.root_element(), "val", "attribute test")
            .unwrap_err();
        let CodecError::ResourceLimit(ref refusal) = error else {
            panic!("expected work refusal")
        };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    }

    #[test]
    fn unsupported_schema_tags_skip_all_attribute_searches() {
        let xml = format!("<Unsupported {}='value' allowmultiplevalues='true' allowconnectedassets='single'/>", "attribute".repeat(256));
        let document = roxmltree::Document::parse(&xml).expect("fixture XML");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("input");
        assert!(super::schema_property(&ctx, document.root_element())
            .expect("unrecognized tag uses only the fixed carrier grammar").is_none());
        assert!(ctx.resource_refusal().is_none());
        let original = ctx.charge_work(1, "fixture refusal").expect_err("zero work");
        let error = super::schema_property(&ctx, document.root_element())
            .expect_err("a skipped tag still preserves the original refusal");
        assert!(matches!((original, error), (CodecError::ResourceLimit(original), CodecError::ResourceLimit(actual)) if actual == original));
    }

    #[test]
    fn schema_carrier_modifiers_preserve_selected_layouts() {
        use super::{Property, ValueLayout};
        let cases = [
            "<Reference allowmultiplevalues='true' allowconnectedassets='single'/>",
            "<TextureURI allowmultiplevalues='true' allowconnectedassets='single'/>",
            "<Float unit='length' allowmultiplevalues='true' allowconnectedassets='single'/>",
            "<Boolean readonly='true' allowmultiplevalues='true'/>",
            "<Distance definitionIteratorData='true'/>",
            "<Color/>",
        ];
        for (case, xml) in cases.into_iter().enumerate() {
            let document = roxmltree::Document::parse(xml).expect("fixture XML");
            with_service_context(&[], |ctx| {
                let property = super::schema_property(ctx, document.root_element()).expect("carrier");
                let standard = super::schema_property(super::admission::StandardAdmission, document.root_element()).expect("standard carrier");
                assert_eq!(format!("{property:?}"), format!("{standard:?}"));
                match case {
                    0 => assert!(matches!(property, Some(Property::Reference { multiple: true }))),
                    1 => assert!(matches!(property, Some(Property::Value { layout: ValueLayout::TextureUri, connectable: true }))),
                    2 => assert!(matches!(property, Some(Property::Value { layout: ValueLayout::Multiple(ValueCarrier::UnitFloat), connectable: true }))),
                    3 | 4 => assert!(property.is_none()),
                    5 => assert!(matches!(property, Some(Property::Value { layout: ValueLayout::Single(ValueCarrier::Color), connectable: false }))),
                    _ => unreachable!(),
                }
            });
        }
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
            super::resolve_inheritance(&ctx, &mut scratch(&ctx), &schemas, &mut std::collections::HashMap::new(), "AChild"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RecursionDepth
        ));
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("fixture fits service profile");
        assert!(super::resolve_inheritance(
            &ctx,
            &mut scratch(&ctx),
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
        let frames = frames_of(&stream).expect("fixture framing is valid");
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &DecodePolicy::service())
            .expect("stream fits service input limit");
        let mut catalog = catalog_of(
            &ctx,
            HashMap::from([
                ("Good".into(), super::Schema::default()),
                (
                    "Unused".into(),
                    super::Schema {
                        base: Some("Absent".into()),
                        ..super::Schema::default()
                    },
                ),
            ]),
            HashMap::new(),
        );
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
                &mut scratch(ctx),
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
    fn schema_archive_size_ceiling_is_a_resource_refusal() {
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
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if matches!(limit.dimension, ResourceDimension::Codec("Protein schema bytes")))
        );
    }

    #[test]
    fn standard_schema_archive_ceiling_matches_the_context_refusal() {
        let mut bytes = schema_archive(&[(
            "Schemas/SimpleSchema.xml", "<Schema><UID val=\"Simple\"/></Schema>",
        )]);
        let size = u32::try_from(super::MAX_SCHEMA_BYTES + 1)
            .expect("local ceiling fits ZIP32").to_le_bytes();
        let central = bytes.windows(4).position(|bytes| bytes == b"PK\x01\x02")
            .expect("central header");
        bytes[central + 24..central + 28].copy_from_slice(&size);
        bytes[22..26].copy_from_slice(&size);
        let error = super::SchemaCatalog::load(super::admission::StandardAdmission, bytes.as_slice())
            .err().expect("standard format ceiling");
        let CodecError::ResourceLimit(expected) = error else { panic!("schema size refusal"); };
        assert_eq!(expected.dimension, ResourceDimension::Codec("Protein schema bytes"));
        assert_eq!(expected.operation, "Protein schema bytes");
        assert_eq!(expected.limit, super::MAX_SCHEMA_BYTES);
        assert_eq!(expected.used, super::MAX_SCHEMA_BYTES);
        assert_eq!(expected.additional, 1);
        let arena = DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("fixture input");
        let error = super::SchemaCatalog::load(&ctx, root).err().expect("context format ceiling");
        assert!(matches!(error, CodecError::ResourceLimit(actual) if actual == expected));
        assert_eq!(ctx.resource_refusal(), Some(expected.clone()));
        assert!(matches!(ctx.charge_work(0, "after schema size refusal"),
            Err(CodecError::ResourceLimit(original)) if original == expected));
    }

    #[test]
    fn schema_catalog_storage_is_scoped_and_released_with_the_catalog() {
        let xml =
            br#"<Schema><UID val="Simple"/><Base val="Root"/><String id="comment"/></Schema>"#;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(xml, &arena, &policy).expect("XML fits input limit");
        let mut storage = scratch(&ctx);
        let mut schemas = HashMap::new();
        super::parse_schema_document(&ctx, &mut storage, "schema", xml, &mut schemas)
            .expect("catalog names and maps use no retained allowance");
        let schema = &schemas["Simple"];
        assert_eq!(schema.base.as_deref(), Some("Root"));
        assert!(schema.properties.contains_key("comment"));
    }

    #[test]
    fn schema_base_selection_preserves_last_valued_declaration() {
        for (bases, expected) in [
            ("", None),
            ("<Base val='First'/><Base/>", Some("First")),
            ("<Base val='First'/><Base val='Final'/><Base/>", Some("Final")),
            ("<Base val='First'/><Base val=''/>", Some("")),
        ] {
            let xml = format!("<Schema><UID val='Simple'/>{bases}</Schema>");
            with_service_context(xml.as_bytes(), |ctx| {
                let mut storage = scratch(ctx);
                let mut schemas = HashMap::new();
                super::parse_schema_document(ctx, &mut storage, "schema", xml.as_bytes(), &mut schemas)
                    .expect("base selection");
                assert_eq!(schemas["Simple"].base.as_deref(), expected);
            });
        }
    }

    #[test]
    fn overwritten_schema_bases_do_not_keep_dead_catalog_storage() {
        let single = "<Schema><UID val='Simple'/><Base val='Final'/></Schema>";
        let replaced = format!("<Schema><UID val='Simple'/><Base val='{}'/><Base val='Final'/><Base/></Schema>", "discarded".repeat(4096));
        let live_storage = |xml: &str, release: bool| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(xml.as_bytes(), &arena, &policy)
                .expect("XML input");
            let mut storage = scratch(&ctx);
            let mut schemas = HashMap::new();
            super::parse_schema_document(&ctx, &mut storage, "schema", xml.as_bytes(), &mut schemas)
                .expect("catalog owns only the selected base");
            assert_eq!(schemas["Simple"].base.as_deref(), Some("Final"));
            if release {
                drop(schemas);
                drop(storage);
            }
            // A refused reservation reports the live usage without admitting
            // probe storage. XML backing has already left the parse boundary.
            let error = ctx.reserve_scoped(policy.limits.max_materialized_bytes + 1, "probe catalog live storage")
                .expect_err("probe exceeds the limit independently of live usage");
            let CodecError::ResourceLimit(limit) = error else { panic!("storage refusal"); };
            assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(ctx.resource_refusal(), Some(limit.clone()));
            limit.used
        };
        let baseline = live_storage(single, false);
        assert!(baseline > 0, "the selected catalog backing remains live");
        assert_eq!(live_storage(&replaced, false), baseline);
        assert_eq!(live_storage(&replaced, true), 0);
    }

    #[test]
    fn schema_xml_tree_refuses_before_parse_allocation() {
        let xml = br#"<Schema><UID val="Simple"/><String id="comment"/></Schema>"#;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes =
            cadmpeg_core::decode::u64_from_index(xml.len()) * 4 - 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(xml, &arena, &policy).expect("XML fits input limit");
        assert!(matches!(
            super::parse_schema_document(&ctx, &mut scratch(&ctx),
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
        policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(xml.len()) * 4;
        let (ctx, _) =
            DecodeContext::from_root_bytes(xml, &arena, &policy).expect("XML fits input limit");
        assert!(matches!(
            super::parse_schema_document(&ctx, &mut scratch(&ctx),
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
            &ctx,
            &mut scratch(&ctx),
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
        policy.limits.max_collection_items = 0;
        for _ in 0..4096 {
            let (ctx, _) =
                DecodeContext::from_root_bytes(xml, &arena, &policy).expect("XML fits input limit");
            let error = super::parse_schema_document(
                &ctx,
                &mut scratch(&ctx),
                "Schemas/SimpleSchema.xml",
                xml,
                &mut std::collections::HashMap::new(),
            )
            .expect_err("schema must refuse at its collection boundary");
            let CodecError::ResourceLimit(limit) = error else {
                panic!("collection refusal expected");
            };
            assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
            assert_eq!(ctx.resource_refusal(), Some(limit));
            let threshold = limit
                .used
                .checked_add(limit.additional)
                .expect("collection threshold");
            assert!(threshold > policy.limits.max_collection_items);
            if limit.operation == "Protein parsed schema" {
                assert_eq!(threshold, policy.limits.max_collection_items + 1);
                return;
            }
            policy.limits.max_collection_items = threshold;
        }
        panic!("Protein parsed schema must refuse after XML tree admission");
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
        // Two six-byte lookups, two visits and four active-set node passes precede the closure.
        let active_node_bytes = 11 * std::mem::size_of::<&str>()
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<usize>();
        policy.limits.max_work_units =
            14 + cadmpeg_core::decode::u64_from_index(4 * active_node_bytes);
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fixture fits input limit");
        assert!(matches!(
            super::resolve_inheritance(&ctx, &mut scratch(&ctx), &schemas, &mut std::collections::HashMap::new(), "Simple"),
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
                .frames()
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
        let (small, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy).expect("input");
        assert!(
            matches!(framing::record_frames_admitted(&small, &stream), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::CollectionItems)
        );
        policy.limits.max_collection_items =
            cadmpeg_core::decode::u64_from_index(record.len() + super::RECORD_MARKER.len()) + 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("stream fits input limit");
        let frames = framing::record_frames_admitted(&ctx, &stream)
            .expect("one frame fits the collection limit");
        let mut catalog = catalog_of(
            &ctx,
            HashMap::new(),
            HashMap::from([("Simple".into(), BTreeMap::new())]),
        );
        let outcome = super::decode_frames_admitted(&ctx, &mut catalog, frames.frames())
            .expect("one outcome needs no second framing pass");
        assert_eq!(outcome.records.len(), 1);
        assert!(matches!(
            ctx.charge_collection_items(1, "probe after frame and outcome"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
        ));
    }

    #[test]
    fn standard_counted_value_ceilings_match_the_context_refusal() {
        let mut repeated = 1_025_u32.to_le_bytes().to_vec();
        repeated.extend(std::iter::repeat_n(0_u8, 1_025 * 4));
        let mut paths = vec![0_u8];
        paths.extend_from_slice(&repeated);
        let mut connections = vec![1_u8, 1];
        connections.extend_from_slice(&repeated);
        for (kind, bytes) in [(0, &repeated), (1, &paths), (2, &connections)] {
            let standard = super::admission::StandardAdmission;
            let error = match kind {
                0 => super::read_property(standard, bytes, &mut 0,
                    super::ValueLayout::Multiple(ValueCarrier::Integer), "values").map(|_| ()),
                1 => read_texture_uri(standard, bytes, &mut 0, "paths").map(|_| ()),
                _ => read_connections(standard, bytes, &mut 0).map(|_| ()),
            }.expect_err("standard format ceiling");
            let CodecError::ResourceLimit(expected) = error else { panic!("format refusal"); };
            assert_eq!(expected.dimension, ResourceDimension::Codec("Protein counted value recovery"));
            assert_eq!(expected.operation, "Protein counted value recovery");
            assert_eq!(expected.limit, super::MAX_RECOVERY_VALUES);
            assert_eq!(expected.used, super::MAX_RECOVERY_VALUES);
            assert_eq!(expected.additional, 1);
            with_service_context(bytes, |ctx| {
                let error = match kind {
                    0 => super::read_property(ctx, bytes, &mut 0,
                        super::ValueLayout::Multiple(ValueCarrier::Integer), "values").map(|_| ()),
                    1 => read_texture_uri(ctx, bytes, &mut 0, "paths").map(|_| ()),
                    _ => read_connections(ctx, bytes, &mut 0).map(|_| ()),
                }.expect_err("context format ceiling");
                assert!(matches!(error, CodecError::ResourceLimit(actual) if actual == expected));
                assert_eq!(ctx.resource_refusal(), Some(expected.clone()));
                assert!(matches!(ctx.charge_work(0, "after format refusal"),
                    Err(CodecError::ResourceLimit(original)) if original == expected));
            });
        }
    }

    #[test]
    fn standard_record_format_ceiling_bypasses_recovery() {
        let properties = || HashMap::from([("Simple".into(), BTreeMap::from([
            ("values".into(), super::Property::Value {
                layout: super::ValueLayout::Multiple(ValueCarrier::Integer), connectable: false,
            }),
        ]))]);
        let stream = |count: u32| {
            let mut record = Vec::new();
            for value in ["Simple", "first", "base", "library"] { push_lp(&mut record, value); }
            record.extend_from_slice(&count.to_le_bytes());
            record.extend(std::iter::repeat_n(0_u8, usize::try_from(count).expect("fixture count") * 4));
            let mut later = Vec::new();
            for value in ["Simple", "later", "base", "library"] { push_lp(&mut later, value); }
            later.extend_from_slice(&0_u32.to_le_bytes());
            paged_stream(&[&record, &later])
        };
        let standard = super::admission::StandardAdmission;
        let bytes = stream(1_025);
        let frames = framing::record_frames_admitted(standard, &bytes).expect("fixture framing");
        let mut catalog = super::SchemaCatalog::empty(standard).expect("standard catalog");
        catalog.properties = properties();
        let error = super::decode_frames_admitted(standard, &mut catalog, frames.frames())
            .expect_err("a format refusal must not become a recovered record");
        let CodecError::ResourceLimit(expected) = error else { panic!("format refusal"); };
        with_service_context(&bytes, |ctx| {
            let mut catalog = catalog_of(ctx, HashMap::new(), properties());
            let error = super::decode_frames_admitted(ctx, &mut catalog, frames.frames())
                .expect_err("context format refusal");
            assert!(matches!(error, CodecError::ResourceLimit(actual) if actual == expected));
            assert_eq!(ctx.resource_refusal(), Some(expected.clone()));
        });
        let bytes = stream(1_024);
        let frames = framing::record_frames_admitted(standard, &bytes).expect("fixture framing");
        let mut catalog = super::SchemaCatalog::empty(standard).expect("standard catalog");
        catalog.properties = properties();
        let outcome = super::decode_frames_admitted(standard, &mut catalog, frames.frames())
            .expect("the exact format ceiling is accepted");
        assert!(outcome.rejected.is_empty());
        assert_eq!(outcome.records.len(), 2);
        for (ordinal, guid) in [(0, "first"), (1, "later")] {
            let record = &outcome.records[ordinal];
            assert_eq!(record.ordinal, u64::try_from(ordinal).expect("fixture ordinal"));
            assert_eq!(record.schema, "Simple");
            assert_eq!(record.guid, guid);
            assert_eq!(record.base, "base");
            assert_eq!(record.asset_lib_id, "library");
        }
        let Some(PropertyValue::Multiple(values)) = outcome.records[0].properties["values"].value() else {
            panic!("repeated integer values");
        };
        assert_eq!(values.values().len(), 1_024);
    }

    #[test]
    fn large_complete_protein_lists_report_a_resource_ceiling() {
        let mut repeated = 1_025_u32.to_le_bytes().to_vec();
        repeated.extend(std::iter::repeat_n(0_u8, 1_025 * 4));
        let mut paths = vec![0_u8];
        paths.extend_from_slice(&repeated);
        let mut connections = vec![1_u8, 1];
        connections.extend_from_slice(&repeated);
        with_service_context(&repeated, |ctx| {
            let error = super::read_property(
                ctx,
                &repeated,
                &mut 0,
                super::ValueLayout::Multiple(ValueCarrier::Integer),
                "values",
            )
            .expect_err("recovery ceiling");
            assert!(
                matches!(error, CodecError::ResourceLimit(limit) if matches!(limit.dimension, ResourceDimension::Codec("Protein counted value recovery")))
            );
        });
        with_service_context(&paths, |ctx| {
            let error =
                read_texture_uri(ctx, &paths, &mut 0, "paths").expect_err("URI recovery ceiling");
            assert!(
                matches!(error, CodecError::ResourceLimit(limit) if matches!(limit.dimension, ResourceDimension::Codec("Protein counted value recovery")))
            );
        });
        with_service_context(&connections, |ctx| {
            let error = read_connections(ctx, &connections, &mut 0)
                .expect_err("connection recovery ceiling");
            assert!(
                matches!(error, CodecError::ResourceLimit(limit) if matches!(limit.dimension, ResourceDimension::Codec("Protein counted value recovery")))
            );
        });
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
                &ctx,
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
            &ctx,
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
            read_texture_uri(&ctx, &bytes, &mut 0, "bitmap"),
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
            read_connections(&ctx, &bytes, &mut 0),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "Protein connected asset GUIDs"
        ));
    }

    #[test]
    fn temporary_record_range_refuses_before_frame_growth() {
        let stream = paged_stream(&[b"frame"]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("stream fits input limit");
        assert!(matches!(
            framing::record_frames_admitted(&ctx, &stream),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::MaterializedBytes
                    && limit.operation == "Protein logical record frame"
        ));
    }

    #[test]
    fn decoded_outcome_refuses_before_rejection_vector_growth() {
        let stream = paged_stream(&[b"bad header"]);
        let frames = frames_of(&stream).expect("fixture framing is valid");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("stream fits input limit");
        let mut catalog = catalog_of(&ctx, HashMap::new(), HashMap::new());
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
        let frames = frames_of(&stream).expect("fixture framing is valid");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 14;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("stream fits input limit");
        let mut catalog = catalog_of(&ctx, HashMap::new(), HashMap::new());
        assert!(matches!(
            super::decode_frames_admitted(&ctx, &mut catalog, &frames),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "Protein rejected record detail"
        ));
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &DecodePolicy::service())
            .expect("stream fits service profile");
        let mut catalog = catalog_of(&ctx, HashMap::new(), HashMap::new());
        assert_eq!(
            super::decode_frames_admitted(&ctx, &mut catalog, &frames)
                .expect("rejection detail fits service profile")
                .rejected
                .len(),
            1
        );
    }

    #[test]
    fn rejected_headers_do_not_copy_valid_prefix_strings() {
        let long = "x".repeat(1_048_576);
        for truncated_field in 1..4 {
            let mut record = Vec::new();
            for _ in 0..truncated_field {
                push_lp(&mut record, &long);
            }
            record.extend_from_slice(&4_u32.to_le_bytes());
            record.push(b'x');
            let stream = paged_stream(&[&record]);
            // Frame fixture setup outside the restricted record context. The
            // record input still contains every large prefix byte.
            let frames = framing::record_frames_admitted(super::admission::StandardAdmission, &stream)
                .expect("fixture framing is valid");
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 256;
            policy.limits.max_materialized_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
                .expect("stream fits input limit");
            let mut catalog = catalog_of(&ctx, HashMap::new(), HashMap::new());
            let outcome = super::decode_frames_admitted(&ctx, &mut catalog, frames.frames())
                .expect("only rejection detail and its outcome slot need ownership");
            assert!(outcome.records.is_empty());
            assert_eq!(outcome.rejected.len(), 1);
            assert_eq!(outcome.rejected[0].ordinal, 0);
            assert_eq!(outcome.rejected[0].detail, "Protein instance record header is malformed");
            assert!(ctx.resource_refusal().is_none(), "truncated field {truncated_field}");
        }
    }

    #[test]
    fn accepted_headers_copy_all_fields_without_scratch_storage() {
        let values = ["Simple", "guid", "base", "library"];
        let mut record = Vec::new();
        for value in values {
            push_lp(&mut record, value);
        }
        let stream = paged_stream(&[&record]);
        let frames = frames_of(&stream).expect("fixture framing is valid");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("stream fits input limit");
        let mut catalog = catalog_of(&ctx, HashMap::new(), HashMap::from([
            ("Simple".into(), BTreeMap::new()),
        ]));
        let outcome = super::decode_frames_admitted(&ctx, &mut catalog, &frames)
            .expect("accepted header bytes are retained output, not scratch");
        assert!(outcome.rejected.is_empty());
        assert_eq!(outcome.records.len(), 1);
        let decoded = &outcome.records[0];
        assert_eq!([decoded.schema.as_str(), decoded.guid.as_str(), decoded.base.as_str(), decoded.asset_lib_id.as_str()], values);
        assert!(decoded.properties.is_empty());
    }

    #[test]
    fn nested_property_resource_refusal_is_not_a_rejected_record() {
        let mut record = Vec::new();
        for value in ["Simple", "guid", "base", ""] {
            push_lp(&mut record, value);
        }
        record.extend_from_slice(&2_u32.to_le_bytes());
        record.extend_from_slice(&1_u32.to_le_bytes());
        record.extend_from_slice(&2_u32.to_le_bytes());
        let stream = paged_stream(&[&record]);
        let frames = frames_of(&stream).expect("fixture framing is valid");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&stream, &arena, &policy)
            .expect("stream fits input limit");
        let mut catalog = catalog_of(
            &ctx,
            HashMap::new(),
            HashMap::from([(
                "Simple".into(),
                BTreeMap::from([(
                    "values".into(),
                    super::Property::Value {
                        layout: super::ValueLayout::Multiple(ValueCarrier::Integer),
                        connectable: false,
                    },
                )]),
            )]),
        );
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
            with_service_context(&bare, |ctx| {
                assert_eq!(
                    read_value(ctx, &bare, &mut at, ValueCarrier::Color, id).unwrap(),
                    PropertyValue::Color(rgba.map(|value| FiniteReal::new(value).expect("finite")))
                );
            });
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
        with_service_context(&connections, |ctx| {
            assert_eq!(
                read_connections(ctx, &connections, &mut at).unwrap(),
                ["first-guid", "second-guid"]
            );
        });
        assert_eq!(at, connections.len());

        let mut at = 0;
        with_service_context(&[0], |ctx| {
            assert!(read_connections(ctx, &[0], &mut at).unwrap().is_empty());
        });
        assert_eq!(at, 1);

        let mut counted = vec![0];
        counted.extend_from_slice(&1u32.to_le_bytes());
        push_lp(&mut counted, "cloud/resource/one");
        let mut at = 0;
        with_service_context(&counted, |ctx| {
            assert_eq!(
                read_texture_uri(ctx, &counted, &mut at, "unifiedbitmap_Bitmap").unwrap(),
                PropertyValue::TextureUri(vec!["cloud/resource/one".into()])
            );
        });
        assert_eq!(at, counted.len());

        let mut single = vec![1];
        push_lp(&mut single, "local_bitmap.png");
        let mut at = 0;
        with_service_context(&single, |ctx| {
            assert_eq!(
                read_texture_uri(ctx, &single, &mut at, "unifiedbitmap_Bitmap").unwrap(),
                PropertyValue::TextureUri(vec!["local_bitmap.png".into()])
            );
        });
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
        let frames = frames_of(&stream).expect("stream is paged");
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].logical_offset(), 0);
        assert_eq!(frames[0].bytes(), [RECORD_MARKER, &first].concat());
        assert_eq!(frames[1].logical_offset(), frames[0].bytes().len());
        assert_eq!(frames[1].bytes(), [RECORD_MARKER, &second].concat());
        assert!(stream.len() > 16 + 3 * PAGE_SIZE, "record one spans pages");

        assert!(frames_of(&[]).is_err());
        let mut truncated = stream.clone();
        truncated.truncate(16 + PAGE_SIZE + 1);
        assert!(frames_of(&truncated).is_err());
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

        let frames = frames_of(&stream).expect("standalone terminal page");
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
