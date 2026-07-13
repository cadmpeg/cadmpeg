// SPDX-License-Identifier: Apache-2.0
//! Versioned FCStd-native records.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Native namespace schema emitted by this crate.
pub const VERSION: u32 = 1;

/// One physical archive span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveSpan {
    /// Stable span identity.
    pub id: String,
    /// Inclusive byte offset.
    pub start: u64,
    /// Exclusive byte offset.
    pub end: u64,
    /// Structural role.
    pub role: String,
    /// Owning entry, when applicable.
    pub entry: Option<String>,
}

/// Metadata read from the persistence document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentFacts {
    /// Stable document-record identity.
    pub id: String,
    /// Persistence schema version.
    pub schema_version: String,
    /// Persistence file version.
    pub file_version: String,
    /// Producing application version, when carried.
    pub program_version: Option<String>,
    /// XML document element name.
    pub root_name: String,
    /// Number of declared application objects.
    pub object_count: usize,
}

/// One declared application object and its persistence state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRecord {
    /// Stable native identity.
    pub id: String,
    /// Persisted object name.
    pub name: String,
    /// Runtime type name.
    pub type_name: String,
    /// Persisted numeric identity, when present.
    pub persistent_id: Option<i64>,
    /// Optional custom view-provider type.
    pub view_type: Option<String>,
    /// Declaration attributes not projected into dedicated fields.
    pub attributes: BTreeMap<String, String>,
    /// Ordered dependency identities.
    pub dependencies: Vec<String>,
    /// Source-order index.
    pub order: usize,
    /// Exact object-data XML, when present.
    pub raw_xml: Option<String>,
}

/// Dynamic-property persistence metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicPropertyMeta {
    /// Display group.
    pub group: String,
    /// Documentation string.
    pub documentation: Option<String>,
    /// Native attribute flags.
    pub attributes: Option<i64>,
    /// Read-only state.
    pub read_only: Option<bool>,
    /// Hidden state.
    pub hidden: Option<bool>,
}

/// One generically recovered ordered link target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkTarget {
    /// Target document identity, when external.
    pub document: Option<String>,
    /// Target object identity, including an explicit empty/null target.
    pub object: Option<String>,
    /// Ordered subelement selectors.
    pub subelements: Vec<String>,
}

/// One property value element retained in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueRecord {
    /// Value element tag.
    pub tag: String,
    /// Ordered position among value elements.
    pub order: usize,
    /// Value attributes.
    pub attributes: BTreeMap<String, String>,
    /// Direct text content.
    pub text: Option<String>,
    /// Exact value-element XML.
    pub raw_xml: String,
}

/// One persisted property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropertyRecord {
    /// Stable native identity.
    pub id: String,
    /// Owning native object or document identity.
    pub owner: String,
    /// Persisted property name.
    pub name: String,
    /// Runtime property type.
    pub type_name: String,
    /// Broad persistence value family selected by the property type.
    pub family: PropertyFamily,
    /// Native status bits.
    pub status: Option<u64>,
    /// Dynamic-property metadata, when carried.
    pub dynamic: Option<DynamicPropertyMeta>,
    /// Source-order index within the owner.
    pub order: usize,
    /// Ordered value elements.
    pub values: Vec<ValueRecord>,
    /// Generically recovered ordered link targets.
    pub links: Vec<LinkTarget>,
    /// Referenced archive entries.
    pub side_entries: Vec<String>,
    /// Exact property XML.
    pub raw_xml: String,
    /// Inclusive byte offset in `Document.xml`.
    pub byte_start: u64,
    /// Exclusive byte offset in `Document.xml`.
    pub byte_end: u64,
}

/// Format-level property value families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropertyFamily {
    /// Boolean, integer, or floating scalar.
    Scalar,
    /// Unit-bearing quantity.
    Quantity,
    /// Enumeration value and option table.
    Enumeration,
    /// Two-, three-, or four-component vector.
    Vector,
    /// Matrix value.
    Matrix,
    /// Placement or transform value.
    Placement,
    /// Text, path, UUID, or byte-string value.
    String,
    /// Key/value map.
    Map,
    /// Ordered list.
    List,
    /// Object, subobject, or external link.
    Link,
    /// Expression bindings.
    Expression,
    /// Embedded or external file reference.
    File,
    /// Inert serialized Python value.
    PythonObject,
    /// Type without a settled family mapping.
    Unknown,
}

/// One logical archive entry and its graph ownership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryRecord {
    /// Stable entry identity.
    pub id: String,
    /// Exact archive entry name.
    pub name: String,
    /// Classified entry role.
    pub role: String,
    /// Logical byte length.
    pub byte_len: u64,
    /// Lowercase SHA-256 of logical bytes.
    pub sha256: String,
    /// Property identities that reference this entry.
    pub referenced_by: Vec<String>,
    /// Complete logical bytes.
    pub data: Vec<u8>,
}

/// One non-overlapping logical-entry byte span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogicalSpan {
    /// Stable span identity.
    pub id: String,
    /// Owning archive entry.
    pub entry: String,
    /// Inclusive logical byte offset.
    pub start: u64,
    /// Exclusive logical byte offset.
    pub end: u64,
    /// `structural`, `typed`, or `named_opaque`.
    pub classification: String,
    /// Native record that owns typed or opaque bytes.
    pub owner: Option<String>,
}
