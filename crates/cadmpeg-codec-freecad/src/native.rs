// SPDX-License-Identifier: Apache-2.0
//! Versioned FCStd-native records.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) fn native_id(kind: &str, key: impl AsRef<str>) -> String {
    format!("fcstd:native:{kind}#{}", encode_id_key(key.as_ref()))
}

pub(crate) fn native_child_id(kind: &str, parent: &str, child: &str) -> String {
    let parent_key = id_key(parent);
    format!("fcstd:native:{kind}#{parent_key}:{}", encode_id_key(child))
}

pub(crate) fn model_id(kind: &str, parent: &str, child: impl AsRef<str>) -> String {
    format!(
        "fcstd:model:{kind}#{}:{}",
        id_key(parent),
        encode_id_key(child.as_ref())
    )
}

pub(crate) fn id_key(id: &str) -> &str {
    id.split_once('#').map_or(id, |(_, key)| key)
}

fn encode_id_key(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/') {
            output.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(output, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{model_id, native_child_id, native_id};

    #[test]
    fn canonical_ids_escape_names_without_aliasing_literal_escapes() {
        assert_eq!(
            native_id("object", "A B#C"),
            "fcstd:native:object#A%20B%23C"
        );
        assert_eq!(native_id("object", "A%20B"), "fcstd:native:object#A%2520B");
        assert_eq!(native_id("object", "A:B"), "fcstd:native:object#A%3AB");
        let property = native_child_id("property", &native_id("object", "A B"), "Shape Value");
        assert_eq!(property, "fcstd:native:property#A%20B:Shape%20Value");
        assert_eq!(
            model_id("body", &property, "root#1"),
            "fcstd:model:body#A%20B:Shape%20Value:root%231"
        );
    }
}

/// Native namespace schema emitted by this crate.
pub const VERSION: u32 = 20;

/// Machine-derived semantic projection census for one design object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignCensusRecord {
    /// Stable census identity derived from the native object.
    pub id: String,
    /// Native application object being classified.
    pub object: String,
    /// Persisted runtime type.
    pub type_name: String,
    /// Neutral history feature projected from the object.
    pub feature: String,
    /// Stable CADIR feature-definition family name.
    pub semantic_kind: String,
    /// Whether the operation has neutral semantics instead of only native retention.
    pub neutral: bool,
    /// Whether topology post-processing composition wraps the operation.
    pub post_processed: bool,
}

/// Machine-derived carrier and topology-family census for one exact shape payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierCensusRecord {
    /// Stable census identity derived from the shape payload.
    pub id: String,
    /// Shape payload being counted.
    pub payload: String,
    /// `text` or `binary` carrier grammar.
    pub form: String,
    /// Grammar version declared by the shape-set header.
    pub topology_version: u8,
    /// Recursive 2D-curve family counts.
    pub curves_2d: BTreeMap<String, u64>,
    /// Recursive 3D-curve family counts.
    pub curves_3d: BTreeMap<String, u64>,
    /// Recursive surface-family counts.
    pub surfaces: BTreeMap<String, u64>,
    /// Topological shape-family counts.
    pub topology: BTreeMap<String, u64>,
    /// Standalone polygon carrier count.
    pub polygons_3d: u64,
    /// Polygon-on-triangulation carrier count.
    pub polygons_on_triangulations: u64,
    /// Triangulation carrier count.
    pub triangulations: u64,
}

/// One support attachment and its distinct persisted frames.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachmentRecord {
    /// Stable attachment identity.
    pub id: String,
    /// Attached application object.
    pub object: String,
    /// Ordered support objects and subelements.
    pub supports: Vec<LinkTarget>,
    /// Persisted attachment-map mode.
    pub map_mode: Option<String>,
    /// Persisted resolved object placement.
    pub placement: Option<[[f64; 4]; 4]>,
    /// Persisted attachment-local offset.
    pub offset: Option<[[f64; 4]; 4]>,
    /// Effective frame used for neutral geometry.
    pub effective_frame: [[f64; 4]; 4],
}

/// Document-level GUI state outside application-object view providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuiDocumentRecord {
    /// Stable GUI document identity.
    pub id: String,
    /// Persisted GUI schema version when declared.
    pub schema_version: Option<u32>,
    /// Exact root attributes.
    pub attributes: BTreeMap<String, String>,
    /// Ordered camera, active-view, clipping, and other document state.
    pub states: Vec<GuiStateRecord>,
}

/// One ordered document-level GUI state element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuiStateRecord {
    /// Stable state identity.
    pub id: String,
    /// Persisted XML element name.
    pub kind: String,
    /// Source order among document-level state elements.
    pub order: usize,
    /// Exact element attributes.
    pub attributes: BTreeMap<String, String>,
    /// Ordered descendant value elements.
    pub values: Vec<ValueRecord>,
    /// Referenced display assets.
    pub side_entries: Vec<String>,
    /// Exact state XML.
    pub raw_xml: String,
    /// Inclusive byte offset in `GuiDocument.xml`.
    pub byte_start: u64,
    /// Exclusive byte offset in `GuiDocument.xml`.
    pub byte_end: u64,
}

/// One semantic annotation object kept distinct from drawing presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticAnnotationRecord {
    /// Stable semantic annotation identity.
    pub id: String,
    /// Owning application object.
    pub object: String,
    /// Persisted annotation runtime type.
    pub kind: String,
    /// Ordered user-visible text fragments.
    pub text: Vec<String>,
    /// Object and subelement references grouped by source property.
    pub references: BTreeMap<String, Vec<LinkTarget>>,
    /// Typed or exactly framed annotation properties grouped by source name.
    pub parameters: BTreeMap<String, String>,
    /// Referenced symbol, image, or other side entries.
    pub side_entries: Vec<String>,
}

/// One application-domain object projected into the L8 census.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationRecord {
    /// Stable census identity.
    pub id: String,
    /// Owning native application object.
    pub object: String,
    /// Runtime type retained exactly.
    pub type_name: String,
    /// Application domain derived from the runtime type prefix.
    pub domain: String,
    /// Ordered owned native property identities.
    pub properties: Vec<String>,
    /// Ordered application-object dependencies.
    pub dependencies: Vec<String>,
    /// Ordered referenced archive assets.
    pub side_entries: Vec<String>,
    /// Whether the object owns serialized code-backed data that must remain inert.
    pub inert_payload: bool,
    /// Source order among application objects.
    pub order: usize,
    /// Inclusive `Document.xml` byte offset of the object-data record.
    pub byte_start: u64,
    /// Exclusive `Document.xml` byte offset of the object-data record.
    pub byte_end: u64,
    /// Exact object-data byte length.
    pub byte_len: u64,
    /// Lowercase SHA-256 of exact object-data bytes.
    pub sha256: String,
    /// Exact retained object-data bytes.
    pub data: Vec<u8>,
    /// Auditable preservation records for every owned property.
    pub property_records: Vec<ApplicationPropertyRecord>,
}

/// Exact application-property preservation record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationPropertyRecord {
    /// Stable preservation identity.
    pub id: String,
    /// Owning application object.
    pub object: String,
    /// Authoritative native property identity.
    pub property: String,
    /// Exact property runtime type.
    pub type_name: String,
    /// Typed persistence family.
    pub family: PropertyFamily,
    /// Source order within the object.
    pub order: usize,
    /// Ordered object and subelement links.
    pub links: Vec<LinkTarget>,
    /// Inclusive `Document.xml` byte offset.
    pub byte_start: u64,
    /// Exclusive `Document.xml` byte offset.
    pub byte_end: u64,
    /// Exact property byte length.
    pub byte_len: u64,
    /// Lowercase SHA-256 of exact property bytes.
    pub sha256: String,
    /// Exact retained property bytes.
    pub data: Vec<u8>,
    /// Complete retained referenced payloads.
    pub payloads: Vec<ApplicationPayloadRecord>,
    /// Whether the property carries inert serialized code-backed data.
    pub inert: bool,
}

/// Complete named payload retained for an application property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationPayloadRecord {
    /// Global native entry identity.
    pub entry: String,
    /// Exact archive entry name.
    pub name: String,
    /// Logical byte length.
    pub byte_len: u64,
    /// Lowercase SHA-256 of complete logical bytes.
    pub sha256: String,
    /// Complete retained logical bytes.
    pub data: Vec<u8>,
}

/// One `TechDraw` page, template, view, dimension, or annotation record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawingRecord {
    /// Stable drawing-record identity.
    pub id: String,
    /// Owning application object.
    pub object: String,
    /// Persisted `TechDraw` runtime type.
    pub kind: String,
    /// Ordered page views for a page record.
    pub views: Vec<String>,
    /// Page template object, when linked.
    pub template: Option<String>,
    /// Ordered source object and subelement references for a view or dimension.
    pub sources: Vec<LinkTarget>,
    /// All drawing relationships grouped by their persisted property name.
    pub relationships: BTreeMap<String, Vec<LinkTarget>>,
    /// Typed scalar/vector/string drawing fields retained by property name.
    pub parameters: BTreeMap<String, String>,
    /// Referenced template or drawing side entries.
    pub side_entries: Vec<String>,
}

/// One assembly joint or grounded-object constraint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JointRecord {
    /// Stable joint identity.
    pub id: String,
    /// Owning application object.
    pub object: String,
    /// Persisted joint family code, or `grounded`.
    pub kind: String,
    /// Ordered connector references with their subelement paths.
    pub references: Vec<LinkTarget>,
    /// Connector-local coordinate frames in connector order.
    pub placements: Vec<[[f64; 4]; 4]>,
    /// Connector attachment-offset frames in connector order.
    pub offsets: Vec<[[f64; 4]; 4]>,
    /// Joint scalar, limit, detach, enable, and suppression properties.
    pub parameters: BTreeMap<String, String>,
}

/// One product container, prototype, or placed link occurrence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductNodeRecord {
    /// Stable record identity.
    pub id: String,
    /// Owning application object.
    pub object: String,
    /// Structural family: `group`, `part`, `link_group`, or `occurrence`.
    pub kind: String,
    /// Ordered contained application objects.
    pub members: Vec<String>,
    /// Linked prototype object for an occurrence.
    pub prototype: Option<String>,
    /// External document token when the prototype is not local.
    pub external_document: Option<String>,
    /// Exact attribute spelling that carried the external document reference.
    pub external_document_attribute: Option<String>,
    /// Local occurrence placement as a row-major affine matrix.
    pub local_transform: Option<[[f64; 4]; 4]>,
    /// Property supplying the placement.
    pub placement_property: Option<String>,
    /// Number of array elements requested by the link.
    pub element_count: Option<i64>,
    /// Whether the prototype transform participates in occurrence placement.
    pub link_transform: Option<bool>,
    /// Ordered per-element placements for a link array.
    pub element_transforms: Vec<[[f64; 4]; 4]>,
    /// Ordered per-element scale vectors for a link array.
    pub element_scales: Vec<[f64; 3]>,
    /// Subelement paths selected on the linked prototype.
    pub linked_subelements: Vec<String>,
    /// Whether the link claims its prototype as a tree child.
    pub claim_child: Option<bool>,
    /// Persisted copy-on-change policy name or numeric code.
    pub copy_on_change: Option<String>,
    /// Original object tracked by copy-on-change.
    pub copy_on_change_source: Option<String>,
    /// Internal ownership group for copy-on-change copies.
    pub copy_on_change_group: Option<String>,
    /// Whether the tracked source has changed.
    pub copy_on_change_touched: Option<bool>,
    /// Base scale vector applied to every occurrence element.
    pub scale: Option<[f64; 3]>,
    /// Per-element visibility overrides in array order.
    pub element_visibility: Vec<bool>,
    /// Explicit per-element application objects in array order.
    pub element_objects: Vec<String>,
}

/// One persisted GUI view provider linked to an application object when available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuiViewProviderRecord {
    /// Stable native identity.
    pub id: String,
    /// Application object identity, or `None` for a GUI-only provider.
    pub object: Option<String>,
    /// Persisted provider name.
    pub name: String,
    /// Persisted tree-expansion state.
    pub expanded: Option<bool>,
    /// Source order in `ViewProviderData`.
    pub order: usize,
    /// Exact provider XML.
    pub raw_xml: String,
}

/// One persisted property owned by a GUI view provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuiPropertyRecord {
    /// Stable native identity.
    pub id: String,
    /// Owning view-provider identity.
    pub owner: String,
    /// Persisted property name.
    pub name: String,
    /// Runtime property type.
    pub type_name: String,
    /// Native status bits.
    pub status: Option<u64>,
    /// Source order within the provider.
    pub order: usize,
    /// Ordered value elements.
    pub values: Vec<ValueRecord>,
    /// Referenced archive entries.
    pub side_entries: Vec<String>,
    /// Exact property XML.
    pub raw_xml: String,
    /// Inclusive byte offset in `GuiDocument.xml`.
    pub byte_start: u64,
    /// Exclusive byte offset in `GuiDocument.xml`.
    pub byte_end: u64,
}

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
    /// Structural document-kind classification.
    pub document_kind: String,
    /// Application domains present in object declarations.
    pub domains: Vec<String>,
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
    /// Inclusive byte offset of object-data XML.
    pub byte_start: Option<u64>,
    /// Exclusive byte offset of object-data XML.
    pub byte_end: Option<u64>,
}

/// One dynamic object extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionRecord {
    /// Stable extension identity.
    pub id: String,
    /// Owning object identity.
    pub owner: String,
    /// Persisted extension name.
    pub name: String,
    /// Runtime extension type.
    pub type_name: String,
    /// Source-order index.
    pub order: usize,
    /// Exact extension XML.
    pub raw_xml: String,
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
    /// Exact attribute spelling that carried the document reference.
    pub document_attribute: Option<String>,
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
    /// Whether this is a status-only transient property declaration.
    pub transient: bool,
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
    /// Exact geometry, mesh, or point carrier.
    Geometry,
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

/// Deterministic whole-archive byte-accounting summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteCoverageRecord {
    /// Stable report identity.
    pub id: String,
    /// Complete physical archive length.
    pub physical_byte_len: u64,
    /// Number of physical partition spans.
    pub physical_span_count: usize,
    /// Number of logical archive entries.
    pub logical_entry_count: usize,
    /// Sum of complete logical entry lengths.
    pub logical_byte_len: u64,
    /// Number of logical partition spans.
    pub logical_span_count: usize,
    /// Byte count for each closed classification.
    pub classification_bytes: BTreeMap<String, u64>,
    /// Sorted logical entries containing named opaque bytes.
    pub named_opaque_entries: Vec<String>,
    /// Whether both physical and logical partitions close exactly.
    pub exact: bool,
}

/// One document-wide persistent string table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringTableRecord {
    /// Stable table identity; the suffix is the zero-based `HasherIndex`.
    pub id: String,
    /// Zero-based document table index referenced by shape properties.
    pub index: usize,
    /// Owning property when the table is serialized beside its first use.
    pub owner_property: Option<String>,
    /// Whether all strings, rather than only marked strings, were persisted.
    pub save_all: bool,
    /// Native hashing threshold.
    pub threshold: i64,
    /// Declared number of serialized entries.
    pub declared_count: usize,
    /// Referenced side entry, or `None` for inline data.
    pub source_entry: Option<String>,
    /// Parsed records in serialized order.
    pub entries: Vec<StringTableEntry>,
}

/// One relative-coded record in a persistent string table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringTableEntry {
    /// Restored numeric string identity.
    pub string_id: i64,
    /// Native flag word.
    pub flags: u64,
    /// Restored referenced string identities.
    pub components: Vec<i64>,
    /// Exact payload following the numeric header.
    pub payload: String,
    /// Exact serialized record without its line terminator.
    pub raw: String,
}

/// One persisted element map owned by an exact-shape property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementMapRecord {
    /// Stable map identity.
    pub id: String,
    /// Owning shape property identity.
    pub property: String,
    /// Version discriminator carried by the shape value.
    pub version: String,
    /// Document string-table index used by mapped names.
    pub hasher_index: Option<usize>,
    /// Referenced side entry, or `None` for inline data.
    pub source_entry: Option<String>,
    /// Native map identity.
    pub map_id: u64,
    /// Declared number of mapped transient elements.
    pub declared_count: usize,
    /// Ordered postfix dictionary.
    pub postfixes: Vec<String>,
    /// Ordered child-map records; the last record is the owning shape map.
    pub maps: Vec<ElementMapNode>,
}

/// One map node, including recursively referenced child maps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementMapNode {
    /// One-based map index in this serialization.
    pub index: usize,
    /// Native node identity.
    pub map_id: u64,
    /// Ordered indexed-element groups.
    pub groups: Vec<ElementMapGroup>,
}

/// Persistent-name chains for one native topology kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementMapGroup {
    /// Native indexed-name prefix such as `Face`, `Edge`, or `Vertex`.
    pub indexed_name: String,
    /// Child-map descriptors retained exactly.
    pub children: Vec<String>,
    /// One entry per transient indexed element, in index order.
    pub names: Vec<Vec<ElementMappedName>>,
}

/// One persistent mapped-name encoding and its neutral topology bindings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementMappedName {
    /// Exact encoded mapped name.
    pub encoded: String,
    /// Decoded base and postfix when all dictionary references are valid.
    pub resolved: Option<String>,
    /// Referenced persistent string identities.
    pub string_ids: Vec<i64>,
    /// Neutral topology ids for every placed occurrence of this element.
    pub topology_ids: Vec<String>,
}
