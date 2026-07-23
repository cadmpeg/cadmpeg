// SPDX-License-Identifier: Apache-2.0
//! CATIA-native ownership and design records retained outside the neutral model.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::catalog;
use crate::container;
#[cfg(test)]
use crate::families::consolidated::records::ConsolidatedEdgeDefinitionData;
use crate::object_graph::{
    self, AliasGroupMembership, AliasLead, HeadToken, ListItem, ObjectPayload, PayloadField,
    PayloadSubtype,
};
use crate::value_block;

/// Current schema version for the CATIA native namespace.
pub const CATIA_NATIVE_VERSION: u32 = 78;

const CATIA_ARENA_NAMES: &[&str] = &[
    "alias_rows",
    "catalog_entries",
    "catalogs",
    "consolidated_edge_nodes",
    "consolidated_edge_runs",
    "consolidated_owner_packets",
    "consolidated_pcurves",
    "consolidated_vertex_identities",
    "design_objects",
    "external_references",
    "finjpl_segments",
    "object_graph_records",
    "object_graphs",
    "preview_images",
    "value_blocks",
    "value_schema_selections",
];

/// Consolidated pcurve framing family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatiaConsolidatedFamily {
    /// A-family frame with a u32 payload length.
    A,
    /// B-family frame with a u8 payload length.
    B,
}

/// Reference dialect used by a consolidated class-`0x62` owner packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatiaOwnerReferenceEncoding {
    /// Strong identities use tagged little-endian `u16` values.
    TaggedU16Strong,
    /// Strong identities use width-coded compact integers.
    WidthCodedStrong,
}

/// Allocation link immediately preceding a consolidated owner packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaOwnerAllocationLink {
    /// Link-record byte offset.
    pub byte_offset: u64,
    /// Complete framed-record byte length.
    pub byte_len: u64,
    /// Width-coded header token.
    pub header_token: u32,
    /// Allocation identity whose successor is the owner's final reference.
    pub target: u32,
}

/// Structurally decoded payload of a class-`0x62` consolidated owner packet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaOwnerNumericTail {
    /// Five-byte class-specific header.
    pub header: [u8; 5],
    /// Four finite binary64 values in serialization order.
    pub scalar64: [f64; 4],
    /// Six finite binary32 values in serialization order.
    pub scalar32: [f32; 6],
}

/// Structurally decoded payload of a class-`0x62` consolidated owner packet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatiaOwnerPacketPayload {
    /// Nine alternating strong/weak identities followed by a fixed numeric tail.
    FixedNine {
        /// Reference encoding selected by the packet.
        reference_encoding: CatiaOwnerReferenceEncoding,
        /// Nine persistent identities in serialization order.
        references: [u32; 9],
        /// Structurally decoded 62-byte class-specific numeric tail.
        numeric_tail: CatiaOwnerNumericTail,
    },
    /// Count-selected persistent identities followed by a nonempty tail.
    Counted {
        /// Persistent identities in serialization order.
        references: Vec<u32>,
        /// Complete nonempty class-specific tail.
        #[serde(with = "cadmpeg_ir::bytes")]
        #[schemars(with = "String")]
        tail: Vec<u8>,
    },
}

#[cfg(test)]
impl CatiaOwnerPacketPayload {
    fn final_reference(&self) -> Option<u32> {
        match self {
            Self::FixedNine { references, .. } => references.last().copied(),
            Self::Counted { references, .. } => references.last().copied(),
        }
    }
}

/// Exact class-`0x62` consolidated owner packet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedOwnerPacket {
    /// Stable source identity.
    pub id: String,
    /// Record byte offset.
    pub byte_offset: u64,
    /// Width-coded header token.
    pub header_token: u32,
    /// Count-specific reference lane and tail.
    pub payload: CatiaOwnerPacketPayload,
    /// Structurally adjacent allocation link, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocation_link: Option<CatiaOwnerAllocationLink>,
}

/// One structurally complete consolidated `A/B:20` pcurve jet whose support
/// identity has not necessarily been resolved to a native surface record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedPcurve {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the framed record.
    pub byte_offset: u64,
    /// Consolidated framing family.
    pub family: CatiaConsolidatedFamily,
    /// Absolute persistent support-surface identity.
    pub support_id: u32,
    /// Parametric curve degree.
    pub degree: u32,
    /// Number of leading extrapolation sites.
    pub extrapolation_sites: u32,
    /// Strictly increasing native parameter sites.
    pub knots: Vec<f64>,
    /// Surface-chart positions at the parameter sites.
    pub points: Vec<[f64; 2]>,
    /// First derivatives at the parameter sites.
    pub first_derivatives: Vec<[f64; 2]>,
    /// Second derivatives at the parameter sites.
    pub second_derivatives: Vec<[f64; 2]>,
    /// Native evaluation interval.
    pub range: [f64; 2],
    /// Bytes following the evaluation interval in the framed payload.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
    pub tail: Vec<u8>,
}

/// One complete consolidated historical edge run referencing two retained
/// pcurve records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedEdgeRun {
    /// Stable native-record identity.
    pub id: String,
    /// Byte offset of the first pcurve frame.
    pub byte_offset: u64,
    /// Retained pcurve identities in serialized side order.
    pub pcurves: [String; 2],
    /// Shared native parameter interval.
    pub parameter_range: [f64; 2],
    /// Shared geometric tolerance.
    pub tolerance: f64,
    /// Exact terminal edge node.
    pub node: String,
    /// Uniquely resolved support carrier for each pcurve side.
    #[serde(default)]
    pub support_bindings: [Option<CatiaConsolidatedSupportBinding>; 2],
    /// Index-aligned 3D loci shared by every resolved support side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_loci: Option<Vec<[f64; 3]>>,
    /// First and last shared loci in physical edge direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_loci: Option<[[f64; 3]; 2]>,
}

/// One structurally complete width-coded class-`0x5e` edge node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedEdgeNode {
    /// Stable native-record identity.
    pub id: String,
    /// Record byte offset.
    pub byte_offset: u64,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent framing flag.
    pub flag: u8,
    /// Width-coded header token.
    pub header_token: u32,
    /// Allocation-local curve-support reference.
    pub curve_ref: u32,
    /// Global native endpoint identities in edge direction.
    pub vertex_refs: [u32; 2],
    /// Retained vertex-identity records in edge direction.
    pub vertices: [String; 2],
    /// Allocation-local endpoint selectors.
    pub parameter_selectors: [u32; 2],
    /// Terminal layout byte.
    pub tail: u8,
    /// Adjacent class-`0x23..=0x25` edge-definition frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<CatiaConsolidatedEdgeDefinition>,
    /// Adjacent oriented uses whose references close on this edge node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uses: Option<CatiaConsolidatedEdgeUses>,
    /// Analytic circle carrier structurally bound by an adjacent six-record run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analytic_circle: Option<CatiaConsolidatedAnalyticCircleCarrier>,
    /// Typed class-`0x18` descriptor bound to a class-`0x25` edge run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class25_descriptor: Option<CatiaConsolidatedClass25Descriptor>,
}

/// Typed class-`0x18` descriptor bound to a class-`0x25` edge definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedClass25Descriptor {
    /// Record byte offset.
    pub byte_offset: u64,
    /// Width-coded allocation identity.
    pub record_id: u32,
    /// Descriptor control byte.
    pub control: u8,
    /// Complete finite scalar lane.
    pub values: Vec<f64>,
}

/// Descriptor and circle records structurally bound to an analytic edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedAnalyticCircleCarrier {
    /// Exact class-`0x18` descriptor frame.
    pub descriptor: CatiaConsolidatedAnalyticCircleDescriptor,
    /// Circle record byte offset.
    pub circle_byte_offset: u64,
    /// Compact persistent circle-record identity.
    pub record_id: u32,
    /// Width-coded circle frame token.
    pub frame_token: u8,
    /// Two center coordinates in the host-implied carrier plane.
    pub center_pair: [f64; 2],
    /// Circle radius in millimetres.
    pub radius: f64,
    /// Arc-length parameter interval.
    pub range: [f64; 2],
    /// Whether the interval spans one complete circumference.
    pub full_circle: bool,
}

/// Exact class-`0x18` descriptor frame attached to an analytic circle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedAnalyticCircleDescriptor {
    /// Record byte offset.
    pub byte_offset: u64,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent framing flag.
    pub flag: u8,
    /// Width-coded header token.
    pub header_token: u32,
    /// Complete class-specific payload.
    pub payload: Vec<u8>,
}

/// Exact class-specific edge-definition frame owned by one consolidated edge node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedEdgeDefinition {
    /// Record byte offset.
    pub byte_offset: u64,
    /// Header-token width in bytes.
    pub width: u8,
    /// Independent framing flag.
    pub flag: u8,
    /// Edge-definition class in `0x23..=0x25`.
    pub class: u8,
    /// Width-coded header token.
    pub header_token: u32,
    /// Complete class-specific payload.
    pub payload: Vec<u8>,
    /// Structurally decoded class-specific payload. Reuses the consolidated
    /// family enum directly: it is serialization-identical (same variant and
    /// field names, no id/offset decoration), so no native restatement is
    /// needed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<crate::families::consolidated::records::ConsolidatedEdgeDefinitionData>,
}

/// Exact oriented-use allocation chain owned by one consolidated edge node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedEdgeUses {
    /// Counted allocation-reference vectors in side order.
    pub references: [[u32; 2]; 2],
    /// Terminal side-use sense bytes in serialized order.
    pub senses: [u8; 2],
}

/// One global endpoint identity retained by consolidated topology edge nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaConsolidatedVertexIdentity {
    /// Stable native-record identity assigned in first-incidence order.
    pub id: String,
    /// Global native endpoint identity.
    pub identity: u32,
    /// Incident consolidated edge nodes in source order.
    pub incident_edge_nodes: Vec<String>,
}

/// Exact carrier selected for one side of a consolidated historical edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CatiaConsolidatedSupportBinding {
    /// Standalone `b2 03 28` cylinder.
    Cylinder {
        /// Carrier record byte offset.
        byte_offset: u64,
    },
    /// Cylinder frame embedded in a `b2 03 60` wrapper.
    EmbeddedCylinder {
        /// Embedded frame byte offset.
        byte_offset: u64,
        /// Enclosing wrapper byte offset.
        wrapper_byte_offset: u64,
    },
    /// Arc-length `b2 03 19` circle.
    Circle {
        /// Carrier record byte offset.
        byte_offset: u64,
    },
    /// `b2 03 29` cone.
    Cone {
        /// Carrier record byte offset.
        byte_offset: u64,
    },
    /// Consolidated NURBS carrier with an optional constant normal offset.
    NurbsCarrier {
        /// Carrier record byte offset.
        byte_offset: u64,
        /// Signed normal offset in millimetres.
        offset: f64,
    },
}

/// One complete outer FINJPL segment retained with its framing identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaFinjplSegment {
    /// Globally unique segment identity.
    pub id: String,
    /// FINJPL marker offset in the complete file.
    pub byte_offset: u64,
    /// Complete segment byte length.
    pub byte_len: u64,
    /// Big-endian segment type word.
    pub type_word: u32,
    /// Structural type family.
    pub family: String,
    /// Stored primary name, when the printable-ASCII name form is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Complete segment bytes from marker through the byte before the next segment.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
    pub data: Vec<u8>,
}

/// One external CATIA document selected by a storage-property record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaExternalReference {
    /// Globally unique reference identity.
    pub id: String,
    /// File offset of the length-prefixed target string.
    pub byte_offset: u64,
    /// Referenced CATIA document name or path.
    pub target: String,
    /// Containing project-flags FINJPL segment.
    pub segment: String,
}

/// One exact JPEG preview from the outer summary-information segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaPreviewImage {
    /// Globally unique preview identity.
    pub id: String,
    /// JPEG SOI byte offset in the complete file.
    pub byte_offset: u64,
    /// Exact encoded length through JPEG EOI.
    pub byte_len: u64,
    /// Pixel width from the JPEG start-of-frame segment.
    pub width: u16,
    /// Pixel height from the JPEG start-of-frame segment.
    pub height: u16,
    /// JPEG component count.
    pub components: u8,
    /// Exact JPEG byte stream.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
    pub data: Vec<u8>,
}

/// One exact outer `01 00 04 00` alias-row core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaAliasRow {
    /// Globally unique alias-row identity.
    pub id: String,
    /// Byte offset of the four-byte alias marker.
    pub byte_offset: u64,
    /// Classification of the preceding four-byte word.
    pub lead: AliasLead,
    /// Complete preceding four-byte word.
    pub lead_raw: u32,
    /// Low 24 bits of the stored tag word.
    pub tag: u32,
    /// Complete stored tag word.
    pub tag_raw: u32,
    /// Single-byte row flag.
    pub flag: u8,
    /// Complete three-byte F1 field.
    pub f1: [u8; 3],
    /// One-based object-graph record ordinal carried by F1.
    pub entity_record_ordinal: u8,
    /// Primary object graph selected by the valid F1 ordinal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_graph: Option<String>,
    /// One-based F1 ordinal resolved to its exact `7C09` record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_record: Option<String>,
    /// Design object owning the selected record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_object: Option<String>,
    /// First trailing fixed-width field.
    pub f2: u32,
    /// Second trailing fixed-width field.
    pub f3: u32,
    /// Group-allocation header immediately preceding this alias core.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<AliasGroupMembership>,
}

/// One exact `7C0B` value block adjacent to its source-schema catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaValueBlock {
    /// Globally unique value-block identity.
    pub id: String,
    /// Byte offset of the `7C0B` marker.
    pub byte_offset: u64,
    /// Complete framed extent including the trailing terminator.
    pub byte_len: u64,
    /// Stored length from the marker through the byte before the terminator.
    pub declared_len: u64,
    /// Object graph ending exactly where this value block begins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_graph: Option<String>,
    /// Source-schema catalog that begins immediately after this block.
    pub catalog: String,
    /// Value payload in serialized order.
    #[serde(with = "cadmpeg_ir::bytes")]
    #[schemars(with = "String")]
    pub payload: Vec<u8>,
    /// Lossless typed fields in payload order.
    #[serde(default)]
    pub fields: Vec<value_block::ValueField>,
    /// Schema selectors in payload order, resolved against the adjacent catalog.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schema_selections: Vec<CatiaValueSchemaSelection>,
}

/// One `0x32` selector from a value block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaValueSchemaSelection {
    /// Globally unique schema-selection identity.
    pub id: String,
    /// Containing [`CatiaValueBlock`] identity.
    pub parent: String,
    /// Byte offset within the value payload.
    pub offset: u64,
    /// Stored zero-based ordinal or terminal absent-schema sentinel.
    pub ordinal: u32,
    /// Selected catalog entry; absent for the terminal sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// UTF-8 source-schema name stored by the selected entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Complete encoded value after this selector and before the next selector.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub encoded_value: Vec<value_block::ValueField>,
}

/// One exact `7C02` source-schema catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaCatalog {
    /// Globally unique catalog identity.
    pub id: String,
    /// Byte offset of the `7C02` marker.
    pub byte_offset: u64,
    /// Total framed byte length.
    pub byte_len: u64,
    /// Stored count, equal to the entry population plus one.
    pub declared_count: u32,
    /// Catalog entries in serialized order.
    #[serde(default)]
    pub entries: Vec<CatiaCatalogEntry>,
}

/// One source-schema name from a [`CatiaCatalog`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaCatalogEntry {
    /// Globally unique catalog-entry identity.
    pub id: String,
    /// Containing [`CatiaCatalog`] identity.
    pub parent: String,
    /// Stable serialized order within the catalog.
    pub ordinal: u32,
    /// Byte offset of the inclusive length field.
    pub byte_offset: u64,
    /// Decoded ASCII schema name.
    pub value: String,
}

/// One outer `7C08` ownership graph in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaObjectGraph {
    /// Globally unique graph identity.
    pub id: String,
    /// Byte offset of the `7C08` root.
    pub byte_offset: u64,
    /// Total framed byte length.
    pub byte_len: u64,
    /// Byte offset of the associated schema catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_byte_offset: Option<u64>,
    /// Associated schema catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<String>,
    /// Consecutive `7C09` records in serialized order.
    #[serde(default)]
    pub records: Vec<CatiaObjectRecord>,
}

/// One `7C09` ownership record and its typed `7C0A` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaObjectRecord {
    /// Globally unique record identity.
    pub id: String,
    /// Containing [`CatiaObjectGraph`] identity.
    pub parent: String,
    /// Design object selected by this record's owner ordinal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_object: Option<String>,
    /// Stable serialized order within the graph.
    pub ordinal: u64,
    /// Byte offset of the `7C09` record.
    pub byte_offset: u64,
    /// Total framed byte length.
    pub byte_len: u64,
    /// First head byte.
    pub lead: u8,
    /// Decoded head tokens in serialized order.
    pub head: Vec<HeadToken>,
    /// First head reference, identifying the owner by one-based record ordinal.
    pub owner_ref: Option<u32>,
    /// Second head reference, identifying the per-file class ordinal.
    pub class_ref: Option<u32>,
    /// UTF-8 class name resolved through the graph's schema catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    /// Exact schema-catalog entry selected by `class_ref`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_entry: Option<String>,
    /// Third head reference, selecting class-specific storage.
    pub storage_ref: Option<u32>,
    /// Typed nested payload.
    pub payload: ObjectPayload,
    /// Structural payload classification.
    pub subtype: PayloadSubtype,
    /// Ordered same-graph payload-reference links.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<CatiaObjectRecordReference>,
}

/// One typed payload reference from a `7C09` record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaObjectRecordReference {
    /// Stored one-based record ordinal.
    pub ordinal: u32,
    /// Exact selected record; absent when the ordinal is outside the graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Design object owning the selected record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_object: Option<String>,
}

/// One exact schema class retained on a grouped design object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaDesignClass {
    /// Selected source-schema entry.
    pub entry: String,
    /// UTF-8 class name stored by the entry.
    pub name: String,
}

/// One serialized design object formed by a shared `7C09` owner ordinal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaDesignObject {
    /// Globally unique design-object identity.
    pub id: String,
    /// Containing [`CatiaObjectGraph`] identity.
    pub parent: String,
    /// Zero-based order of this owner group by its first field in the graph.
    pub ordinal: u64,
    /// Byte offset of the first field carrying this owner ordinal.
    pub first_field_byte_offset: u64,
    /// One-based owner ordinal stored by every field record.
    pub owner_ordinal: u32,
    /// Record selected by `owner_ordinal` when it lies inside the graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_record: Option<String>,
    /// Design object whose field set contains `owner_record`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_design_object: Option<String>,
    /// Exact class of a separator-form owner declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_class: Option<CatiaDesignClass>,
    /// Class-specific storage selector of a separator-form owner declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_storage_ref: Option<u32>,
    /// Field records carrying this owner ordinal, in serialized order.
    pub fields: Vec<String>,
    /// Distinct exact field classes, in first field order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_classes: Vec<CatiaDesignClass>,
    /// Referenced design objects, in first field-reference order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub object_references: Vec<String>,
}

fn design_objects(graphs: &[CatiaObjectGraph]) -> Vec<CatiaDesignObject> {
    graphs
        .iter()
        .flat_map(|graph| {
            let mut fields = Vec::<(u32, Vec<&CatiaObjectRecord>)>::new();
            let mut owner_indices = HashMap::<u32, usize>::new();
            for record in &graph.records {
                if let Some(owner) = record.owner_ref {
                    let index = owner_indices.get(&owner).copied().unwrap_or_else(|| {
                        let index = fields.len();
                        fields.push((owner, Vec::new()));
                        owner_indices.insert(owner, index);
                        index
                    });
                    fields[index].1.push(record);
                }
            }
            fields
                .into_iter()
                .enumerate()
                .map(move |(ordinal, (owner_ordinal, records))| {
                    let owner_record = object_record_index(owner_ordinal, graph.records.len())
                        .and_then(|index| graph.records.get(index));
                    let mut referenced_owners = Vec::new();
                    for reference in records
                        .iter()
                        .flat_map(|record| payload_references(&record.payload))
                    {
                        let target_owner = object_record_index(reference, graph.records.len())
                            .and_then(|index| graph.records.get(index))
                            .and_then(|record| record.owner_ref);
                        if let Some(target_owner) = target_owner.filter(|target| {
                            *target != owner_ordinal
                                && owner_indices.contains_key(target)
                                && !referenced_owners.contains(target)
                        }) {
                            referenced_owners.push(target_owner);
                        }
                    }
                    CatiaDesignObject {
                        id: design_object_id(graph.byte_offset, owner_ordinal),
                        parent: graph.id.clone(),
                        ordinal: ordinal as u64,
                        first_field_byte_offset: records[0].byte_offset,
                        owner_ordinal,
                        owner_record: owner_record.map(|record| record.id.clone()),
                        owner_design_object: owner_record
                            .and_then(|record| record.owner_ref)
                            .filter(|owner| {
                                *owner != owner_ordinal && owner_indices.contains_key(owner)
                            })
                            .map(|owner| design_object_id(graph.byte_offset, owner)),
                        owner_class: owner_record
                            .filter(|record| record_has_separator_roles(record))
                            .and_then(|record| {
                                Some(CatiaDesignClass {
                                    entry: record.class_entry.clone()?,
                                    name: record.class_name.clone()?,
                                })
                            }),
                        owner_storage_ref: owner_record
                            .filter(|record| record_has_separator_roles(record))
                            .and_then(|record| record.storage_ref),
                        fields: records.iter().map(|record| record.id.clone()).collect(),
                        field_classes: records
                            .iter()
                            .filter_map(|record| {
                                Some(CatiaDesignClass {
                                    entry: record.class_entry.clone()?,
                                    name: record.class_name.clone()?,
                                })
                            })
                            .fold(Vec::new(), |mut classes, class| {
                                if !classes.contains(&class) {
                                    classes.push(class);
                                }
                                classes
                            }),
                        object_references: referenced_owners
                            .into_iter()
                            .map(|owner| design_object_id(graph.byte_offset, owner))
                            .collect(),
                    }
                })
        })
        .collect()
}

fn record_has_separator_roles(record: &CatiaObjectRecord) -> bool {
    matches!(record.head.get(1), Some(HeadToken::Separator))
}

fn object_record_index(ordinal: u32, record_count: usize) -> Option<usize> {
    let index = usize::try_from(ordinal).ok()?.checked_sub(1)?;
    (index < record_count).then_some(index)
}

fn design_object_id(graph_offset: u64, owner_ordinal: u32) -> String {
    format!("catia:outer:design-object#{graph_offset:010}-{owner_ordinal:010}")
}

fn payload_references(payload: &ObjectPayload) -> impl Iterator<Item = u32> + '_ {
    payload.fields.iter().flat_map(|field| match field {
        PayloadField::Reference { value, .. } => vec![*value],
        PayloadField::List {
            declared_count,
            items,
            ..
        } if usize::try_from(*declared_count).ok() == Some(items.len()) => items
            .iter()
            .filter_map(|item| match item {
                ListItem::Reference(value) => Some(*value),
                ListItem::Atom(_) => None,
            })
            .collect(),
        PayloadField::List { .. } => Vec::new(),
        PayloadField::Atom { .. }
        | PayloadField::Scalar { .. }
        | PayloadField::Blob { .. }
        | PayloadField::BulkTable { .. }
        | PayloadField::Sentinel { .. }
        | PayloadField::Terminator => Vec::new(),
    })
}

fn resolved_payload_references(
    payload: &ObjectPayload,
    record_ids: &[String],
    record_design_objects: &[Option<String>],
) -> Vec<CatiaObjectRecordReference> {
    payload_references(payload)
        .map(|ordinal| {
            let index = object_record_index(ordinal, record_ids.len());
            CatiaObjectRecordReference {
                ordinal,
                target: index.and_then(|index| record_ids.get(index)).cloned(),
                design_object: index
                    .and_then(|index| record_design_objects.get(index))
                    .cloned()
                    .flatten(),
            }
        })
        .collect()
}

/// CATIA-native records retained outside the format-neutral model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaNative {
    /// Schema version this namespace was written under.
    pub version: u32,
    /// Exact outer alias-row cores in source order.
    #[serde(default)]
    pub alias_rows: Vec<CatiaAliasRow>,
    /// Framed source-schema name catalogs.
    #[serde(default)]
    pub catalogs: Vec<CatiaCatalog>,
    /// Structurally complete consolidated edge nodes.
    #[serde(default)]
    pub consolidated_edge_nodes: Vec<CatiaConsolidatedEdgeNode>,
    /// Complete consolidated historical edge runs.
    #[serde(default)]
    pub consolidated_edge_runs: Vec<CatiaConsolidatedEdgeRun>,
    /// Exact consolidated owner packets and their allocation links.
    #[serde(default)]
    pub consolidated_owner_packets: Vec<CatiaConsolidatedOwnerPacket>,
    /// Consolidated pcurve jets retained before support resolution.
    #[serde(default)]
    pub consolidated_pcurves: Vec<CatiaConsolidatedPcurve>,
    /// Global endpoint identities and their consolidated edge incidence.
    #[serde(default)]
    pub consolidated_vertex_identities: Vec<CatiaConsolidatedVertexIdentity>,
    /// Design objects grouped by their serialized owner ordinal.
    #[serde(default)]
    pub design_objects: Vec<CatiaDesignObject>,
    /// External CATIA document references in source order.
    #[serde(default)]
    pub external_references: Vec<CatiaExternalReference>,
    /// Complete bounded outer FINJPL segments.
    #[serde(default)]
    pub finjpl_segments: Vec<CatiaFinjplSegment>,
    /// Outer ownership graphs.
    #[serde(default)]
    pub object_graphs: Vec<CatiaObjectGraph>,
    /// Exact JPEG previews extracted from summary-information records.
    #[serde(default)]
    pub preview_images: Vec<CatiaPreviewImage>,
    /// Framed value blocks adjacent to source-schema catalogs.
    #[serde(default)]
    pub value_blocks: Vec<CatiaValueBlock>,
}

impl Default for CatiaNative {
    fn default() -> Self {
        Self {
            version: CATIA_NATIVE_VERSION,
            alias_rows: Vec::new(),
            catalogs: Vec::new(),
            consolidated_edge_nodes: Vec::new(),
            consolidated_edge_runs: Vec::new(),
            consolidated_owner_packets: Vec::new(),
            consolidated_pcurves: Vec::new(),
            consolidated_vertex_identities: Vec::new(),
            design_objects: Vec::new(),
            external_references: Vec::new(),
            finjpl_segments: Vec::new(),
            object_graphs: Vec::new(),
            preview_images: Vec::new(),
            value_blocks: Vec::new(),
        }
    }
}

fn consolidated_pcurves(bytes: &[u8]) -> Vec<CatiaConsolidatedPcurve> {
    let mut pcurves = crate::families::a5a8::records::a5_pcurves(bytes)
        .into_iter()
        .map(|pcurve| (pcurve, CatiaConsolidatedFamily::A))
        .chain(
            crate::families::b2::records::b2_pcurves(bytes)
                .into_iter()
                .map(|pcurve| (pcurve, CatiaConsolidatedFamily::B)),
        )
        .collect::<Vec<_>>();
    pcurves.sort_by_key(|(pcurve, _)| pcurve.pos);
    pcurves
        .into_iter()
        .enumerate()
        .map(|(index, (pcurve, family))| CatiaConsolidatedPcurve {
            id: format!("catia:consolidated:pcurve#{index}"),
            byte_offset: pcurve.pos as u64,
            family,
            support_id: pcurve.support_id,
            degree: pcurve.degree,
            extrapolation_sites: pcurve.extrapolation_sites,
            knots: pcurve.knots,
            points: pcurve.points,
            first_derivatives: pcurve.first_derivatives,
            second_derivatives: pcurve.second_derivatives,
            range: pcurve.range,
            tail: pcurve.tail,
        })
        .collect()
}

fn consolidated_owner_packets(bytes: &[u8]) -> Vec<CatiaConsolidatedOwnerPacket> {
    let links = crate::families::b2::records::b2_linked_owners(bytes)
        .into_iter()
        .map(|linked| (linked.owner.pos, linked.link))
        .chain(
            crate::families::b2::records::b2_linked_counted_owners(bytes)
                .into_iter()
                .map(|linked| (linked.owner.pos, linked.link)),
        )
        .collect::<HashMap<_, _>>();
    let fixed = crate::families::b2::records::b2_owner_packets(bytes);
    let fixed_positions = fixed
        .iter()
        .map(|packet| packet.pos)
        .collect::<HashSet<_>>();
    let mut packets = fixed
        .into_iter()
        .map(|packet| {
            (
                packet.pos,
                packet.header_token,
                CatiaOwnerPacketPayload::FixedNine {
                    reference_encoding: match packet.reference_encoding {
                        crate::families::b2::records::B2OwnerReferenceEncoding::TaggedU16Strong => {
                            CatiaOwnerReferenceEncoding::TaggedU16Strong
                        }
                        crate::families::b2::records::B2OwnerReferenceEncoding::WidthCodedStrong => {
                            CatiaOwnerReferenceEncoding::WidthCodedStrong
                        }
                    },
                    references: packet.references,
                    numeric_tail: CatiaOwnerNumericTail {
                        header: packet.numeric_tail.header,
                        scalar64: packet.numeric_tail.scalar64,
                        scalar32: packet.numeric_tail.scalar32,
                    },
                },
            )
        })
        .chain(
            crate::families::b2::records::b2_counted_owners(bytes)
                .into_iter()
                .filter(|packet| !fixed_positions.contains(&packet.pos))
                .map(|packet| {
                    (
                        packet.pos,
                        packet.header_token,
                        CatiaOwnerPacketPayload::Counted {
                            references: packet.references,
                            tail: packet.tail,
                        },
                    )
                }),
        )
        .collect::<Vec<_>>();
    packets.sort_by_key(|(pos, _, _)| *pos);
    packets
        .into_iter()
        .map(
            |(pos, header_token, payload)| CatiaConsolidatedOwnerPacket {
                id: format!("catia:consolidated:owner-packet#{pos:010}"),
                byte_offset: pos as u64,
                header_token,
                payload,
                allocation_link: links.get(&pos).map(|link| CatiaOwnerAllocationLink {
                    byte_offset: link.pos as u64,
                    byte_len: (pos - link.pos) as u64,
                    header_token: link.header_token,
                    target: link.target,
                }),
            },
        )
        .collect()
}

fn consolidated_edge_runs(
    bytes: &[u8],
    pcurves: &[CatiaConsolidatedPcurve],
    nodes: &[CatiaConsolidatedEdgeNode],
) -> Vec<CatiaConsolidatedEdgeRun> {
    let pcurve_ids = pcurves
        .iter()
        .map(|pcurve| (pcurve.byte_offset, pcurve.id.clone()))
        .collect::<HashMap<_, _>>();
    let resolved = crate::families::consolidated::records::resolve_consolidated_edge_blocks(bytes)
        .into_iter()
        .map(|block| (block.block.pcurves[0].pos, block))
        .collect::<HashMap<_, _>>();
    let nodes_by_offset = nodes
        .iter()
        .map(|node| (node.byte_offset, node))
        .collect::<HashMap<_, _>>();
    crate::families::consolidated::records::consolidated_topology_edge_runs(bytes)
        .into_iter()
        .filter_map(|run| {
            if !run.edge.co_parametric || !run.identity_chain_consistent {
                return None;
            }
            let pcurve_offsets = run.edge.pcurves.each_ref().map(|pcurve| pcurve.pos as u64);
            Some((run, pcurve_offsets))
        })
        .enumerate()
        .filter_map(|(index, (run, pcurve_offsets))| {
            let resolved = resolved.get(&run.edge.pcurves[0].pos);
            let node = nodes_by_offset.get(&(run.node.pos as u64))?;
            node.uses.as_ref()?;
            Some(CatiaConsolidatedEdgeRun {
                id: format!("catia:consolidated:edge-run#{index}"),
                byte_offset: pcurve_offsets[0],
                pcurves: [
                    pcurve_ids.get(&pcurve_offsets[0])?.clone(),
                    pcurve_ids.get(&pcurve_offsets[1])?.clone(),
                ],
                parameter_range: run.edge.parameters.range,
                tolerance: run.edge.parameters.tolerance,
                node: node.id.clone(),
                support_bindings: resolved.map_or([None, None], |resolved| {
                    resolved
                        .supports
                        .each_ref()
                        .map(|binding| binding.as_ref().map(native_consolidated_support_binding))
                }),
                shared_loci: resolved
                    .and_then(|resolved| resolved.shared_loci.as_ref())
                    .map(|points| points.iter().map(point_coordinates).collect()),
                endpoint_loci: resolved
                    .and_then(|resolved| resolved.endpoint_loci.as_ref())
                    .map(|points| points.map(|point| point_coordinates(&point))),
            })
        })
        .collect()
}

fn consolidated_edge_nodes(bytes: &[u8]) -> Vec<CatiaConsolidatedEdgeNode> {
    let frames = crate::wire::records::consolidated_records(bytes)
        .into_iter()
        .filter(|record| {
            record.family == crate::wire::records::ConsolidatedFamily::B && record.class == 0x5e
        })
        .map(|record| (record.range.start, (record.width, record.flag)))
        .collect::<HashMap<_, _>>();
    let use_runs = crate::families::consolidated::records::consolidated_edge_use_runs(bytes)
        .into_iter()
        .filter_map(|run| {
            if !run.identity_chain_consistent {
                return None;
            }
            Some((
                run.node.pos,
                (
                    native_consolidated_edge_uses(&run.uses)?,
                    run.definition.map(native_consolidated_edge_definition),
                ),
            ))
        })
        .collect::<HashMap<_, _>>();
    let analytic_circles =
        crate::families::consolidated::records::consolidated_analytic_circle_edge_runs(bytes)
            .into_iter()
            .filter(|run| run.identity_chain_consistent)
            .map(|run| {
                (
                    run.node.pos,
                    native_consolidated_analytic_circle(&run.descriptor, &run.circle),
                )
            })
            .collect::<HashMap<_, _>>();
    let class25_descriptors =
        crate::families::consolidated::records::consolidated_class25_edge_runs(bytes)
            .into_iter()
            .filter(|run| run.identity_chain_consistent)
            .map(|run| {
                (
                    run.node.pos,
                    CatiaConsolidatedClass25Descriptor {
                        byte_offset: run.descriptor.pos as u64,
                        record_id: run.descriptor.record_id,
                        control: run.descriptor.control,
                        values: run.descriptor.values,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
    crate::families::b2::records::b2_edge_nodes(bytes)
        .into_iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let (width, flag) = frames.get(&node.pos)?;
            Some(CatiaConsolidatedEdgeNode {
                id: format!("catia:consolidated:edge-node#{index}"),
                byte_offset: node.pos as u64,
                width: *width,
                flag: *flag,
                header_token: node.header_token,
                curve_ref: node.curve_ref,
                vertex_refs: [node.start_vertex_ref, node.end_vertex_ref],
                vertices: [String::new(), String::new()],
                parameter_selectors: [node.start_parameter_ref, node.end_parameter_ref],
                tail: node.tail,
                definition: use_runs.get(&node.pos).and_then(|(_, value)| value.clone()),
                uses: use_runs.get(&node.pos).map(|(value, _)| value.clone()),
                analytic_circle: analytic_circles.get(&node.pos).cloned(),
                class25_descriptor: class25_descriptors.get(&node.pos).cloned(),
            })
        })
        .collect()
}

fn native_consolidated_analytic_circle(
    descriptor: &crate::families::consolidated::records::ConsolidatedAnalyticCircleDescriptor,
    circle: &crate::families::b2::records::B2Circle,
) -> CatiaConsolidatedAnalyticCircleCarrier {
    CatiaConsolidatedAnalyticCircleCarrier {
        descriptor: CatiaConsolidatedAnalyticCircleDescriptor {
            byte_offset: descriptor.pos as u64,
            width: descriptor.width,
            flag: descriptor.flag,
            header_token: descriptor.header_token,
            payload: descriptor.payload.clone(),
        },
        circle_byte_offset: circle.pos as u64,
        record_id: circle.record_id,
        frame_token: circle.frame_token,
        center_pair: circle.center_pair,
        radius: circle.radius,
        range: circle.range,
        full_circle: circle.full_circle,
    }
}

fn native_consolidated_edge_definition(
    definition: crate::families::consolidated::records::ConsolidatedEdgeDefinition,
) -> CatiaConsolidatedEdgeDefinition {
    CatiaConsolidatedEdgeDefinition {
        byte_offset: definition.pos as u64,
        width: definition.width,
        flag: definition.flag,
        class: definition.class,
        header_token: definition.header_token,
        payload: definition.payload,
        data: definition.data,
    }
}

fn native_consolidated_edge_uses(
    uses: &[crate::families::b2::records::B2UseMetadata; 2],
) -> Option<CatiaConsolidatedEdgeUses> {
    let references = uses
        .iter()
        .map(|use_| use_.references.as_deref()?.try_into().ok())
        .collect::<Option<Vec<[u32; 2]>>>()?
        .try_into()
        .ok()?;
    let senses = uses
        .each_ref()
        .map(|use_| match use_.sense? {
            crate::families::b2::records::B2UseSense::Sense84 => Some(0x84),
            crate::families::b2::records::B2UseSense::Sense88 => Some(0x88),
        })
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    (senses == [0x88, 0x84]).then_some(CatiaConsolidatedEdgeUses { references, senses })
}

fn consolidated_vertex_identities(
    nodes: &mut [CatiaConsolidatedEdgeNode],
) -> Vec<CatiaConsolidatedVertexIdentity> {
    let mut identities = Vec::<CatiaConsolidatedVertexIdentity>::new();
    let mut identity_indices = HashMap::<u32, usize>::new();
    for node in nodes {
        for (endpoint, identity) in node.vertex_refs.into_iter().enumerate() {
            let index = *identity_indices.entry(identity).or_insert_with(|| {
                let index = identities.len();
                identities.push(CatiaConsolidatedVertexIdentity {
                    id: format!("catia:consolidated:vertex-identity#{index}"),
                    identity,
                    incident_edge_nodes: Vec::new(),
                });
                index
            });
            let vertex = &mut identities[index];
            node.vertices[endpoint].clone_from(&vertex.id);
            if vertex.incident_edge_nodes.last() != Some(&node.id) {
                vertex.incident_edge_nodes.push(node.id.clone());
            }
        }
    }
    identities
}

fn point_coordinates(point: &cadmpeg_ir::math::Point3) -> [f64; 3] {
    [point.x, point.y, point.z]
}

fn native_consolidated_support_binding(
    binding: &crate::families::consolidated::records::ConsolidatedSupportBinding,
) -> CatiaConsolidatedSupportBinding {
    match binding {
        crate::families::consolidated::records::ConsolidatedSupportBinding::Cylinder { pos } => {
            CatiaConsolidatedSupportBinding::Cylinder {
                byte_offset: *pos as u64,
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::EmbeddedCylinder {
            pos,
            wrapper_pos,
        } => CatiaConsolidatedSupportBinding::EmbeddedCylinder {
            byte_offset: *pos as u64,
            wrapper_byte_offset: *wrapper_pos as u64,
        },
        crate::families::consolidated::records::ConsolidatedSupportBinding::Circle { pos } => {
            CatiaConsolidatedSupportBinding::Circle {
                byte_offset: *pos as u64,
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::Cone { pos } => {
            CatiaConsolidatedSupportBinding::Cone {
                byte_offset: *pos as u64,
            }
        }
        crate::families::consolidated::records::ConsolidatedSupportBinding::NurbsCarrier {
            pos,
            offset,
        } => CatiaConsolidatedSupportBinding::NurbsCarrier {
            byte_offset: *pos as u64,
            offset: *offset,
        },
    }
}

#[cfg(test)]
fn validate_consolidated_pcurves(
    pcurves: &[CatiaConsolidatedPcurve],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, pcurve) in pcurves.iter().enumerate() {
        let expected_id = format!("catia:consolidated:pcurve#{index}");
        let count = pcurve.knots.len();
        if pcurve.id != expected_id
            || pcurve.degree != 5
            || count < 2
            || pcurve.points.len() != count
            || pcurve.first_derivatives.len() != count
            || pcurve.second_derivatives.len() != count
            || pcurve.knots.windows(2).any(|pair| pair[0] >= pair[1])
            || pcurve.range[0] >= pcurve.range[1]
            || pcurve
                .knots
                .iter()
                .chain(pcurve.points.iter().flatten())
                .chain(pcurve.first_derivatives.iter().flatten())
                .chain(pcurve.second_derivatives.iter().flatten())
                .chain(&pcurve.range)
                .any(|value| !value.is_finite())
            || !matches!(pcurve.tail.as_slice(), [0x07] | [0x07, 0x00])
            || index > 0 && pcurves[index - 1].byte_offset >= pcurve.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated pcurve `{}` is structurally invalid",
                pcurve.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_owner_packets(
    packets: &[CatiaConsolidatedOwnerPacket],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for (index, packet) in packets.iter().enumerate() {
        let valid_link = packet.allocation_link.is_none_or(|link| {
            link.byte_offset.checked_add(link.byte_len) == Some(packet.byte_offset)
                && link.target.checked_add(1) == packet.payload.final_reference()
        });
        let valid_payload = match &packet.payload {
            CatiaOwnerPacketPayload::FixedNine { numeric_tail, .. } => {
                numeric_tail.header[0] == 0x84
                    && matches!(numeric_tail.header[1], 0x41 | 0xc1)
                    && numeric_tail.header[4] == 0x0d
                    && numeric_tail.scalar64.iter().all(|value| value.is_finite())
                    && numeric_tail.scalar32.iter().all(|value| value.is_finite())
            }
            CatiaOwnerPacketPayload::Counted { references, tail } => {
                !references.is_empty() && !tail.is_empty()
            }
        };
        if packet.id != format!("catia:consolidated:owner-packet#{:010}", packet.byte_offset)
            || !valid_payload
            || !valid_link
            || index > 0 && packets[index - 1].byte_offset >= packet.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated owner packet `{}` is structurally invalid",
                packet.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_consolidated_edge_runs(
    runs: &[CatiaConsolidatedEdgeRun],
    pcurves: &[CatiaConsolidatedPcurve],
    nodes: &[CatiaConsolidatedEdgeNode],
    vertex_identities: &[CatiaConsolidatedVertexIdentity],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    let pcurves = pcurves
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), pcurve))
        .collect::<HashMap<_, _>>();
    let nodes_by_id = nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<HashMap<_, _>>();
    let mut run_nodes = HashSet::new();
    for (index, node) in nodes.iter().enumerate() {
        let token_limit = 1u32.checked_shl(u32::from(node.width) * 8);
        let uses_valid = node.uses.as_ref().is_none_or(|uses| {
            node.curve_ref
                .checked_sub(2)
                .zip(node.curve_ref.checked_sub(1))
                .is_some_and(|(first, second)| {
                    uses.references == [[first, second], [second, node.curve_ref]]
                })
                && uses.senses == [0x88, 0x84]
                && node.parameter_selectors == [2, 1]
        });
        let definition_valid = node.definition.as_ref().is_none_or(|definition| {
            let token_limit = 1u32.checked_shl(u32::from(definition.width) * 8);
            let expected_data =
                crate::families::consolidated::records::consolidated_edge_definition_data(
                    definition.class,
                    &definition.payload,
                );
            node.uses.is_some()
                && matches!(definition.width, 1..=3)
                && matches!(definition.flag, 0x03 | 0x13 | 0x83)
                && matches!(definition.class, 0x23..=0x25)
                && token_limit.is_some_and(|limit| definition.header_token < limit)
                && !definition.payload.is_empty()
                && definition.byte_offset < node.byte_offset
                && definition.data == expected_data
        });
        let analytic_circle_valid = node.analytic_circle.as_ref().is_none_or(|carrier| {
            let definition = node.definition.as_ref();
            node.uses.is_some()
                && definition.is_some_and(|definition| {
                    definition.class == 0x23
                        && matches!(
                            definition.data,
                            Some(ConsolidatedEdgeDefinitionData::Scalar {
                                ref values,
                                ..
                            }) if values.len() == 8
                        )
                        && carrier.descriptor.byte_offset < carrier.circle_byte_offset
                        && carrier.circle_byte_offset < definition.byte_offset
                })
                && matches!(carrier.descriptor.width, 1..=3)
                && matches!(carrier.descriptor.flag, 0x03 | 0x13 | 0x83)
                && 1u32
                    .checked_shl(u32::from(carrier.descriptor.width) * 8)
                    .is_some_and(|limit| carrier.descriptor.header_token < limit)
                && !carrier.descriptor.payload.is_empty()
                && carrier.center_pair.iter().all(|value| value.is_finite())
                && carrier.radius.is_finite()
                && carrier.radius > 0.0
                && carrier.range.iter().all(|value| value.is_finite())
                && carrier.range[0] < carrier.range[1]
        });
        let class25_descriptor_valid = node.class25_descriptor.as_ref().is_none_or(|descriptor| {
            node.uses.is_some()
                && node.definition.as_ref().is_some_and(|definition| {
                    definition.class == 0x25
                        && matches!(
                            definition.data,
                            Some(
                                ConsolidatedEdgeDefinitionData::Scalar25 { .. }
                                    | ConsolidatedEdgeDefinitionData::SegmentedScalar25 { .. }
                            )
                        )
                        && descriptor.byte_offset < definition.byte_offset
                })
                && matches!(descriptor.control, 0x02 | 0x0a)
                && matches!(descriptor.values.len(), 2 | 3)
                && descriptor.values.iter().all(|value| value.is_finite())
        });
        if node.id != format!("catia:consolidated:edge-node#{index}")
            || !matches!(node.width, 1..=3)
            || !matches!(node.flag, 0x03 | 0x13 | 0x83)
            || token_limit.is_some_and(|limit| node.header_token >= limit)
            || !uses_valid
            || !definition_valid
            || !analytic_circle_valid
            || !class25_descriptor_valid
            || index > 0 && nodes[index - 1].byte_offset >= node.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge node `{}` is structurally invalid",
                node.id
            )));
        }
    }
    for (index, run) in runs.iter().enumerate() {
        let expected_id = format!("catia:consolidated:edge-run#{index}");
        let pcurve_offsets = run
            .pcurves
            .each_ref()
            .map(|id| pcurves.get(id.as_str()).map(|pcurve| pcurve.byte_offset));
        let pcurve_ranges = run
            .pcurves
            .each_ref()
            .map(|id| pcurves.get(id.as_str()).map(|pcurve| pcurve.range));
        let Some(node) = nodes_by_id.get(run.node.as_str()) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge run `{}` references missing node `{}`",
                run.id, run.node
            )));
        };
        if !run_nodes.insert(run.node.as_str()) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge node `{}` belongs to multiple runs",
                run.node
            )));
        }
        let loci_valid = run.shared_loci.as_ref().map_or_else(
            || run.endpoint_loci.is_none(),
            |loci| {
                loci.len() >= 2
                    && loci.iter().flatten().all(|value| value.is_finite())
                    && run.endpoint_loci
                        == loci
                            .first()
                            .copied()
                            .zip(loci.last().copied())
                            .map(|(first, last)| [first, last])
            },
        );
        let bindings_valid = run.support_bindings.iter().flatten().all(|binding| {
            !matches!(
                binding,
                CatiaConsolidatedSupportBinding::NurbsCarrier { offset, .. }
                    if !offset.is_finite()
            )
        });
        if run.id != expected_id
            || pcurve_offsets[0] != Some(run.byte_offset)
            || pcurve_offsets[1].is_none()
            || pcurve_offsets[0] >= pcurve_offsets[1]
            || pcurve_offsets[1].is_some_and(|offset| offset >= node.byte_offset)
            || pcurve_ranges != [Some(run.parameter_range), Some(run.parameter_range)]
            || run.parameter_range[0] >= run.parameter_range[1]
            || !run.parameter_range.iter().all(|value| value.is_finite())
            || !run.tolerance.is_finite()
            || run.tolerance < 0.0
            || node.uses.is_none()
            || !matches!(node.tail, 0x01 | 0x21)
            || !bindings_valid
            || !loci_valid
            || index > 0 && runs[index - 1].byte_offset >= run.byte_offset
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "consolidated edge run `{}` is structurally invalid",
                run.id
            )));
        }
    }
    let mut expected_nodes = nodes.to_vec();
    let expected_identities = consolidated_vertex_identities(&mut expected_nodes);
    if expected_nodes != nodes || expected_identities != vertex_identities {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "consolidated vertex identities disagree with edge incidence".to_string(),
        ));
    }
    Ok(())
}

fn contains_extent(
    owner_start: usize,
    owner_len: usize,
    candidate_start: usize,
    candidate_len: usize,
) -> bool {
    owner_start < candidate_start
        && owner_start
            .checked_add(owner_len)
            .zip(candidate_start.checked_add(candidate_len))
            .is_some_and(|(owner_end, candidate_end)| candidate_end <= owner_end)
}

fn extents_overlap(first_start: u64, first_len: u64, second_start: u64, second_len: u64) -> bool {
    first_start
        .checked_add(first_len)
        .zip(second_start.checked_add(second_len))
        .is_some_and(|(first_end, second_end)| first_start < second_end && second_start < first_end)
}

fn finjpl_family(kind: container::FinjplKind) -> &'static str {
    match kind {
        container::FinjplKind::Storage => "storage",
        container::FinjplKind::ProjectFlags => "project-flags",
        container::FinjplKind::Other => "other",
    }
}

fn preview_views(segments: &[CatiaFinjplSegment]) -> Vec<CatiaPreviewImage> {
    segments
        .iter()
        .flat_map(|segment| {
            container::preview_images(&segment.data)
                .into_iter()
                .filter_map(move |preview| {
                    Some((
                        segment
                            .byte_offset
                            .checked_add(u64::try_from(preview.range.start).ok()?)?,
                        preview,
                        segment,
                    ))
                })
        })
        .enumerate()
        .map(
            |(index, (byte_offset, preview, segment))| CatiaPreviewImage {
                id: format!("catia:outer:preview#{index}"),
                byte_offset,
                byte_len: (preview.range.end - preview.range.start) as u64,
                width: preview.width,
                height: preview.height,
                components: preview.components,
                data: segment.data[preview.range].to_vec(),
            },
        )
        .collect()
}

fn external_reference_views(segments: &[CatiaFinjplSegment]) -> Vec<CatiaExternalReference> {
    segments
        .iter()
        .flat_map(|segment| {
            container::external_references(&segment.data)
                .into_iter()
                .filter_map(move |reference| {
                    Some((
                        segment
                            .byte_offset
                            .checked_add(u64::try_from(reference.offset).ok()?)?,
                        reference,
                        segment,
                    ))
                })
        })
        .enumerate()
        .map(
            |(index, (byte_offset, reference, segment))| CatiaExternalReference {
                id: format!("catia:outer:external-reference#{index}"),
                byte_offset,
                target: reference.target,
                segment: segment.id.clone(),
            },
        )
        .collect()
}

#[cfg(test)]
fn validate_native_links(
    aliases: &[CatiaAliasRow],
    catalogs: &[CatiaCatalog],
    graphs: &[CatiaObjectGraph],
    segments: &[CatiaFinjplSegment],
    value_blocks: &[CatiaValueBlock],
) -> Result<(), cadmpeg_ir::NativeConvertError> {
    for catalog in catalogs {
        let count_width = if catalog.declared_count <= 0x50 { 1 } else { 2 };
        let Some(mut expected_offset) = catalog.byte_offset.checked_add(6 + count_width) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` has an overflowing extent",
                catalog.id
            )));
        };
        let catalog_end = catalog.byte_offset.checked_add(catalog.byte_len);
        if catalog.id != format!("catia:outer:catalog#{:010}", catalog.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` has an invalid source identity",
                catalog.id
            )));
        }
        for (index, entry) in catalog.entries.iter().enumerate() {
            let next_offset = catalog
                .entries
                .get(index + 1)
                .map(|next| next.byte_offset)
                .or(catalog_end);
            let encoded_len = next_offset.and_then(|next| next.checked_sub(entry.byte_offset));
            let value_len = u64::try_from(entry.value.len()).ok();
            if entry.byte_offset != expected_offset
                || entry.id != format!("catia:outer:catalog-entry#{:010}", entry.byte_offset)
                || !encoded_len.zip(value_len).is_some_and(|(encoded, value)| {
                    matches!(encoded.checked_sub(value), Some(1 | 5))
                })
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "catalog entry `{}` has an invalid source extent",
                    entry.id
                )));
            }
            expected_offset = next_offset.expect("validated catalog end");
        }
        if Some(expected_offset) != catalog_end {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog `{}` entries do not cover its frame",
                catalog.id
            )));
        }
    }
    for (index, segment) in segments.iter().enumerate() {
        let parsed = container::finjpl_segments(&segment.data, 0, segment.data.len());
        let expected_id = format!("catia:outer:finjpl#{index}");
        if segment.id != expected_id
            || u64::try_from(segment.data.len()).ok() != Some(segment.byte_len)
            || segment.byte_offset.checked_add(segment.byte_len).is_none()
            || !matches!(parsed.as_slice(), [parsed]
                if parsed.range == (0..segment.data.len())
                    && parsed.type_word == segment.type_word
                    && finjpl_family(parsed.kind) == segment.family
                    && parsed.name == segment.name)
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "FINJPL segment `{}` has an invalid retained view",
                segment.id
            )));
        }
    }
    if segments
        .windows(2)
        .any(|pair| pair[0].byte_offset.checked_add(pair[0].byte_len) != Some(pair[1].byte_offset))
    {
        return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
            "CATIA FINJPL segment extents are not contiguous".to_string(),
        ));
    }
    for block in value_blocks {
        if block.id != format!("catia:outer:value-block#{:010}", block.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid source identity",
                block.id
            )));
        }
        let Some(catalog) = catalogs.iter().find(|catalog| catalog.id == block.catalog) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` references missing catalog `{}`",
                block.id, block.catalog
            )));
        };
        if block.byte_offset.checked_add(block.byte_len) != Some(catalog.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` is not adjacent to catalog `{}`",
                block.id, block.catalog
            )));
        }
        let payload_len = u64::try_from(block.payload.len()).ok();
        if block.declared_len.checked_add(1) != Some(block.byte_len)
            || payload_len.and_then(|len| len.checked_add(6)) != Some(block.declared_len)
            || value_block::tokenize(&block.payload) != block.fields
            || value_schema_selections(&block.id, block.byte_offset, &block.fields, catalog)
                != block.schema_selections
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid derived view",
                block.id
            )));
        }
        let mut adjacent_graphs = graphs.iter().filter(|graph| {
            graph.byte_offset.checked_add(graph.byte_len) == Some(block.byte_offset)
        });
        let adjacent_graph = adjacent_graphs.next();
        if adjacent_graphs.next().is_some()
            || block.object_graph.as_deref() != adjacent_graph.map(|graph| graph.id.as_str())
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "value block `{}` has an invalid adjacent graph link",
                block.id
            )));
        }
    }
    for graph in graphs {
        let Some(graph_end) = graph.byte_offset.checked_add(graph.byte_len) else {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an overflowing extent",
                graph.id
            )));
        };
        let mut expected_record_offset = graph.byte_offset.checked_add(6);
        if graph.id != format!("catia:outer:object-graph#{:010}", graph.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an invalid source identity",
                graph.id
            )));
        }
        for record in &graph.records {
            if Some(record.byte_offset) != expected_record_offset
                || record.id != format!("catia:outer:object-record#{:010}", record.byte_offset)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object record `{}` has an invalid source extent",
                    record.id
                )));
            }
            expected_record_offset = record.byte_offset.checked_add(record.byte_len);
        }
        if expected_record_offset != Some(graph_end) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` records do not cover its frame",
                graph.id
            )));
        }
        let mut candidates = catalogs
            .iter()
            .filter(|catalog| catalog.byte_offset == graph_end)
            .chain(
                value_blocks
                    .iter()
                    .filter(|block| block.byte_offset == graph_end)
                    .filter_map(|block| {
                        catalogs.iter().find(|catalog| catalog.id == block.catalog)
                    }),
            );
        let catalog = candidates.next();
        if candidates.next().is_some()
            || graph.catalog_byte_offset != catalog.map(|catalog| catalog.byte_offset)
            || graph.catalog.as_deref() != catalog.map(|catalog| catalog.id.as_str())
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object graph `{}` has an invalid schema-catalog link",
                graph.id
            )));
        }
        for record in &graph.records {
            let expected_class = catalog.and_then(|catalog| {
                usize::try_from(record.class_ref?).ok().and_then(|ordinal| {
                    catalog
                        .entries
                        .get(ordinal)
                        .map(|entry| (entry.id.as_str(), entry.value.as_str()))
                })
            });
            if record.class_entry.as_deref() != expected_class.map(|(entry, _)| entry)
                || record.class_name.as_deref() != expected_class.map(|(_, value)| value)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "object record `{}` has an invalid schema class",
                    record.id
                )));
            }
        }
    }
    let maximum_records = graphs
        .iter()
        .map(|graph| graph.records.len())
        .max()
        .unwrap_or(0);
    let mut primary_graphs = graphs
        .iter()
        .filter(|graph| graph.records.len() == maximum_records);
    let primary_graph = match (primary_graphs.next(), primary_graphs.next()) {
        (Some(graph), None) => Some(graph),
        _ => None,
    };
    for alias in aliases {
        if alias.id != format!("catia:outer:alias-row#{:010}", alias.byte_offset) {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "alias row `{}` has an invalid source identity",
                alias.id
            )));
        }
        let expected = usize::from(alias.entity_record_ordinal)
            .checked_sub(1)
            .and_then(|index| {
                let graph = primary_graph?;
                let record = graph.records.get(index)?;
                Some((
                    graph.id.as_str(),
                    record.id.as_str(),
                    record.design_object.as_deref(),
                ))
            });
        let valid = expected.map_or_else(
            || {
                alias.object_graph.is_none()
                    && alias.object_record.is_none()
                    && alias.design_object.is_none()
            },
            |(graph, record, object)| {
                alias.object_graph.as_deref() == Some(graph)
                    && alias.object_record.as_deref() == Some(record)
                    && alias.design_object.as_deref() == object
            },
        );
        if !valid {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "alias row `{}` has invalid graph, record, or design-object links",
                alias.id
            )));
        }
        if let Some(group) = &alias.group {
            if group.target_slot != (u32::from(alias.f1[2]) | ((alias.f2 & 0x00ff_ffff) << 8))
                || !object_graph::is_alias_group_storage_prefix(&group.storage_prefix)
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "alias row `{}` has invalid group storage",
                    alias.id
                )));
            }
        }
    }
    Ok(())
}

impl CatiaNative {
    /// Decode CATIA-native records directly from the complete file image.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Self {
        let mut parsed_catalogs = catalog::parse(bytes);
        let mut alias_rows = object_graph::surface_aliases(bytes)
            .into_iter()
            .map(CatiaAliasRow::from)
            .collect::<Vec<_>>();
        let mut parsed_object_graphs = object_graph::parse_all(bytes);
        let mut parsed_value_blocks = value_block::parse(bytes);
        parsed_value_blocks.retain(|block| {
            !parsed_object_graphs.iter().any(|graph| {
                contains_extent(graph.pos, graph.total_len, block.pos, block.total_len)
            })
        });
        parsed_object_graphs.retain(|graph| {
            !parsed_value_blocks.iter().any(|block| {
                contains_extent(block.pos, block.total_len, graph.pos, graph.total_len)
            })
        });
        parsed_catalogs.retain(|catalog| {
            !parsed_object_graphs.iter().any(|graph| {
                contains_extent(graph.pos, graph.total_len, catalog.pos, catalog.total_len)
            }) && !parsed_value_blocks.iter().any(|block| {
                contains_extent(block.pos, block.total_len, catalog.pos, catalog.total_len)
            })
        });
        let catalogs: Vec<CatiaCatalog> = parsed_catalogs
            .into_iter()
            .map(CatiaCatalog::from)
            .collect();
        let mut object_graphs: Vec<CatiaObjectGraph> = parsed_object_graphs
            .into_iter()
            .map(CatiaObjectGraph::from)
            .collect();
        for graph in &mut object_graphs {
            let catalog = graph.catalog_byte_offset.and_then(|offset| {
                catalogs
                    .iter()
                    .find(|catalog| catalog.byte_offset == offset)
            });
            graph.catalog = catalog.map(|catalog| catalog.id.clone());
            for record in &mut graph.records {
                record.class_entry = record.class_ref.and_then(|ordinal| {
                    usize::try_from(ordinal)
                        .ok()
                        .and_then(|ordinal| catalog?.entries.get(ordinal))
                        .map(|entry| entry.id.clone())
                });
            }
        }
        alias_rows.retain(|row| {
            let row_start = row.byte_offset.saturating_sub(4);
            !object_graphs
                .iter()
                .any(|graph| extents_overlap(row_start, 24, graph.byte_offset, graph.byte_len))
                && !parsed_value_blocks.iter().any(|block| {
                    extents_overlap(row_start, 24, block.pos as u64, block.total_len as u64)
                })
                && !catalogs.iter().any(|catalog| {
                    extents_overlap(row_start, 24, catalog.byte_offset, catalog.byte_len)
                })
        });
        let design_objects = design_objects(&object_graphs);
        let maximum_records = object_graphs
            .iter()
            .map(|graph| graph.records.len())
            .max()
            .unwrap_or(0);
        let mut primary_graphs = object_graphs
            .iter()
            .filter(|graph| graph.records.len() == maximum_records);
        if let (Some(graph), None) = (primary_graphs.next(), primary_graphs.next()) {
            for row in &mut alias_rows {
                let Some(index) = usize::from(row.entity_record_ordinal).checked_sub(1) else {
                    continue;
                };
                let Some(record) = graph.records.get(index) else {
                    continue;
                };
                row.object_graph = Some(graph.id.clone());
                row.object_record = Some(record.id.clone());
                row.design_object.clone_from(&record.design_object);
            }
        }
        let value_blocks = parsed_value_blocks
            .into_iter()
            .filter_map(|block| {
                let catalog_pos = block.pos + block.total_len;
                let catalog = catalogs
                    .iter()
                    .find(|catalog| catalog.byte_offset == catalog_pos as u64)?;
                let object_graph = object_graphs.iter().find(|graph| {
                    graph
                        .byte_offset
                        .checked_add(graph.byte_len)
                        .is_some_and(|end| end == block.pos as u64)
                });
                Some(CatiaValueBlock::from_parts(block, catalog, object_graph))
            })
            .collect();
        let finjpl_segments = container::finjpl_segments(bytes, 0, bytes.len())
            .into_iter()
            .enumerate()
            .map(|(index, segment)| CatiaFinjplSegment {
                id: format!("catia:outer:finjpl#{index}"),
                byte_offset: segment.range.start as u64,
                byte_len: (segment.range.end - segment.range.start) as u64,
                type_word: segment.type_word,
                family: finjpl_family(segment.kind).to_string(),
                name: segment.name,
                data: bytes[segment.range].to_vec(),
            })
            .collect::<Vec<_>>();
        let preview_images = preview_views(&finjpl_segments);
        let external_references = external_reference_views(&finjpl_segments);
        let consolidated_owner_packets = consolidated_owner_packets(bytes);
        let consolidated_pcurves = consolidated_pcurves(bytes);
        let mut consolidated_edge_nodes = consolidated_edge_nodes(bytes);
        let consolidated_edge_runs =
            consolidated_edge_runs(bytes, &consolidated_pcurves, &consolidated_edge_nodes);
        let consolidated_vertex_identities =
            consolidated_vertex_identities(&mut consolidated_edge_nodes);
        Self {
            version: CATIA_NATIVE_VERSION,
            alias_rows,
            catalogs,
            consolidated_edge_nodes,
            consolidated_edge_runs,
            consolidated_owner_packets,
            consolidated_pcurves,
            consolidated_vertex_identities,
            design_objects,
            external_references,
            finjpl_segments,
            object_graphs,
            preview_images,
            value_blocks,
        }
    }

    /// Load the typed CATIA namespace from generic native arenas.
    #[cfg(test)]
    pub fn load(
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        let mut catalogs: Vec<CatiaCatalog> = namespace.arena_as("catalogs")?;
        let entries: Vec<CatiaCatalogEntry> = namespace.arena_as("catalog_entries")?;
        let catalog_ids = catalogs
            .iter()
            .map(|catalog| catalog.id.as_str())
            .collect::<HashSet<_>>();
        if catalog_ids.len() != catalogs.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA catalog identity".to_string(),
            ));
        }
        if let Some(entry) = entries
            .iter()
            .find(|entry| !catalog_ids.contains(entry.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "catalog entry `{}` references missing catalog `{}`",
                entry.id, entry.parent
            )));
        }
        for catalog in &mut catalogs {
            catalog.entries = entries
                .iter()
                .filter(|entry| entry.parent == catalog.id)
                .cloned()
                .collect();
            catalog.entries.sort_by_key(|entry| entry.ordinal);
            if u32::try_from(catalog.entries.len())
                .ok()
                .and_then(|count| count.checked_add(1))
                != Some(catalog.declared_count)
                || catalog
                    .entries
                    .iter()
                    .enumerate()
                    .any(|(ordinal, entry)| usize::try_from(entry.ordinal).ok() != Some(ordinal))
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "catalog `{}` has an invalid entry sequence",
                    catalog.id
                )));
            }
        }
        let mut graphs: Vec<CatiaObjectGraph> = namespace.arena_as("object_graphs")?;
        let records: Vec<CatiaObjectRecord> = namespace.arena_as("object_graph_records")?;
        let graph_ids = graphs
            .iter()
            .map(|graph| graph.id.as_str())
            .collect::<HashSet<_>>();
        if graph_ids.len() != graphs.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA object-graph identity".to_string(),
            ));
        }
        if let Some(record) = records
            .iter()
            .find(|record| !graph_ids.contains(record.parent.as_str()))
        {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                "object record `{}` references missing graph `{}`",
                record.id, record.parent
            )));
        }
        for graph in &mut graphs {
            graph.records = records
                .iter()
                .filter(|record| record.parent == graph.id)
                .cloned()
                .collect();
            graph.records.sort_by_key(|record| record.ordinal);
            let record_ids = graph
                .records
                .iter()
                .map(|record| record.id.clone())
                .collect::<Vec<_>>();
            let record_design_objects = graph
                .records
                .iter()
                .map(|record| record.design_object.clone())
                .collect::<Vec<_>>();
            for (ordinal, record) in graph.records.iter().enumerate() {
                let expected_design_object = record
                    .owner_ref
                    .map(|owner| design_object_id(graph.byte_offset, owner));
                if usize::try_from(record.ordinal).ok() != Some(ordinal)
                    || record.design_object != expected_design_object
                    || record.references
                        != resolved_payload_references(
                            &record.payload,
                            &record_ids,
                            &record_design_objects,
                        )
                {
                    return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                        "object graph `{}` has an invalid record sequence",
                        graph.id
                    )));
                }
            }
        }
        let mut value_blocks: Vec<CatiaValueBlock> = namespace.arena_as("value_blocks")?;
        let value_schema_selections: Vec<CatiaValueSchemaSelection> =
            namespace.arena_as("value_schema_selections")?;
        let value_block_ids = value_blocks
            .iter()
            .map(|block| block.id.clone())
            .collect::<HashSet<_>>();
        if value_block_ids.len() != value_blocks.len() {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "duplicate CATIA value-block identity".to_string(),
            ));
        }
        let mut selections_by_block = HashMap::<String, Vec<CatiaValueSchemaSelection>>::new();
        for selection in value_schema_selections {
            if !value_block_ids.contains(&selection.parent) {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(format!(
                    "value selection `{}` references missing block `{}`",
                    selection.id, selection.parent
                )));
            }
            selections_by_block
                .entry(selection.parent.clone())
                .or_default()
                .push(selection);
        }
        for block in &mut value_blocks {
            block.schema_selections = selections_by_block.remove(&block.id).unwrap_or_default();
            block
                .schema_selections
                .sort_by_key(|selection| selection.offset);
        }
        let design_objects = design_objects(&graphs);
        if namespace.arenas.contains_key("design_objects") {
            let stored: Vec<CatiaDesignObject> = namespace.arena_as("design_objects")?;
            let stored_by_id = stored
                .iter()
                .map(|object| (object.id.as_str(), object))
                .collect::<HashMap<_, _>>();
            if stored_by_id.len() != stored.len()
                || stored.len() != design_objects.len()
                || design_objects
                    .iter()
                    .any(|object| stored_by_id.get(object.id.as_str()).copied() != Some(object))
            {
                return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                    "stored CATIA design objects disagree with their object graph".to_string(),
                ));
            }
        }
        let mut finjpl_segments: Vec<CatiaFinjplSegment> =
            if namespace.arenas.contains_key("finjpl_segments") {
                namespace.arena_as("finjpl_segments")?
            } else {
                Vec::new()
            };
        finjpl_segments.sort_by_key(|segment| segment.byte_offset);
        let mut external_references: Vec<CatiaExternalReference> =
            if namespace.arenas.contains_key("external_references") {
                namespace.arena_as("external_references")?
            } else {
                Vec::new()
            };
        external_references.sort_by_key(|reference| reference.byte_offset);
        let expected_external_references = external_reference_views(&finjpl_segments);
        if external_references != expected_external_references {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "stored CATIA external references disagree with their project-flags segments"
                    .to_string(),
            ));
        }
        let external_references = expected_external_references;
        let mut preview_images: Vec<CatiaPreviewImage> =
            if namespace.arenas.contains_key("preview_images") {
                namespace.arena_as("preview_images")?
            } else {
                Vec::new()
            };
        preview_images.sort_by_key(|preview| preview.byte_offset);
        let expected_preview_images = preview_views(&finjpl_segments);
        if preview_images != expected_preview_images {
            return Err(cadmpeg_ir::NativeConvertError::InvalidOwner(
                "stored CATIA previews disagree with their summary segments".to_string(),
            ));
        }
        let preview_images = expected_preview_images;
        let alias_rows: Vec<CatiaAliasRow> = namespace.arena_as("alias_rows")?;
        let mut consolidated_owner_packets: Vec<CatiaConsolidatedOwnerPacket> =
            namespace.arena_as("consolidated_owner_packets")?;
        consolidated_owner_packets.sort_by_key(|packet| packet.byte_offset);
        validate_consolidated_owner_packets(&consolidated_owner_packets)?;
        let mut consolidated_pcurves: Vec<CatiaConsolidatedPcurve> =
            namespace.arena_as("consolidated_pcurves")?;
        consolidated_pcurves.sort_by_key(|pcurve| pcurve.byte_offset);
        validate_consolidated_pcurves(&consolidated_pcurves)?;
        let mut consolidated_edge_runs: Vec<CatiaConsolidatedEdgeRun> =
            namespace.arena_as("consolidated_edge_runs")?;
        consolidated_edge_runs.sort_by_key(|run| run.byte_offset);
        let mut consolidated_edge_nodes: Vec<CatiaConsolidatedEdgeNode> =
            namespace.arena_as("consolidated_edge_nodes")?;
        consolidated_edge_nodes.sort_by_key(|node| node.byte_offset);
        let consolidated_vertex_identities: Vec<CatiaConsolidatedVertexIdentity> =
            namespace.arena_as("consolidated_vertex_identities")?;
        validate_consolidated_edge_runs(
            &consolidated_edge_runs,
            &consolidated_pcurves,
            &consolidated_edge_nodes,
            &consolidated_vertex_identities,
        )?;
        validate_native_links(
            &alias_rows,
            &catalogs,
            &graphs,
            &finjpl_segments,
            &value_blocks,
        )?;
        Ok(Self {
            version: namespace.version,
            alias_rows,
            catalogs,
            consolidated_edge_nodes,
            consolidated_edge_runs,
            consolidated_owner_packets,
            consolidated_pcurves,
            consolidated_vertex_identities,
            design_objects,
            external_references,
            finjpl_segments,
            object_graphs: graphs,
            preview_images,
            value_blocks,
        })
    }

    /// Store the typed CATIA namespace into generic native arenas.
    #[cfg(test)]
    pub fn store(
        &self,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        namespace.version = CATIA_NATIVE_VERSION;
        let catalogs = self
            .catalogs
            .iter()
            .cloned()
            .map(|mut catalog| {
                catalog.entries.clear();
                catalog
            })
            .collect::<Vec<_>>();
        let entries = self
            .catalogs
            .iter()
            .flat_map(|catalog| catalog.entries.iter().cloned())
            .collect::<Vec<_>>();
        let graphs = self
            .object_graphs
            .iter()
            .cloned()
            .map(|mut graph| {
                graph.records.clear();
                graph
            })
            .collect::<Vec<_>>();
        let records = self
            .object_graphs
            .iter()
            .flat_map(|graph| graph.records.iter().cloned())
            .collect::<Vec<_>>();
        let value_blocks = self
            .value_blocks
            .iter()
            .cloned()
            .map(|mut block| {
                block.schema_selections.clear();
                block
            })
            .collect::<Vec<_>>();
        let value_schema_selections = self
            .value_blocks
            .iter()
            .flat_map(|block| block.schema_selections.iter().cloned())
            .collect::<Vec<_>>();
        namespace.set_arena("catalogs", &catalogs)?;
        namespace.set_arena("consolidated_edge_nodes", &self.consolidated_edge_nodes)?;
        namespace.set_arena("consolidated_edge_runs", &self.consolidated_edge_runs)?;
        namespace.set_arena(
            "consolidated_owner_packets",
            &self.consolidated_owner_packets,
        )?;
        namespace.set_arena("consolidated_pcurves", &self.consolidated_pcurves)?;
        namespace.set_arena(
            "consolidated_vertex_identities",
            &self.consolidated_vertex_identities,
        )?;
        namespace.set_arena("design_objects", &self.design_objects)?;
        namespace.set_arena("external_references", &self.external_references)?;
        namespace.set_arena("finjpl_segments", &self.finjpl_segments)?;
        namespace.set_arena("alias_rows", &self.alias_rows)?;
        namespace.set_arena("catalog_entries", &entries)?;
        namespace.set_arena("object_graphs", &graphs)?;
        namespace.set_arena("object_graph_records", &records)?;
        namespace.set_arena("preview_images", &self.preview_images)?;
        namespace.set_arena("value_blocks", &value_blocks)?;
        namespace.set_arena("value_schema_selections", &value_schema_selections)?;
        debug_assert!(CATIA_ARENA_NAMES
            .iter()
            .all(|name| namespace.arenas.contains_key(*name)));
        Ok(())
    }

    /// Store this namespace while moving child arenas out of their typed owners.
    ///
    /// Decode paths use this form so large object graphs are not cloned while
    /// converting them into generic native records.
    pub fn store_owned(
        self,
        namespace: &mut cadmpeg_ir::NativeNamespace,
    ) -> Result<(), cadmpeg_ir::NativeConvertError> {
        let Self {
            version: _,
            alias_rows,
            mut catalogs,
            consolidated_edge_nodes,
            consolidated_edge_runs,
            consolidated_owner_packets,
            consolidated_pcurves,
            consolidated_vertex_identities,
            design_objects,
            external_references,
            finjpl_segments,
            mut object_graphs,
            preview_images,
            mut value_blocks,
        } = self;
        let entries = catalogs
            .iter_mut()
            .flat_map(|catalog| std::mem::take(&mut catalog.entries))
            .collect::<Vec<_>>();
        let records = object_graphs
            .iter_mut()
            .flat_map(|graph| std::mem::take(&mut graph.records))
            .collect::<Vec<_>>();
        let value_schema_selections = value_blocks
            .iter_mut()
            .flat_map(|block| std::mem::take(&mut block.schema_selections))
            .collect::<Vec<_>>();

        namespace.version = CATIA_NATIVE_VERSION;
        namespace.set_arena("catalogs", &catalogs)?;
        namespace.set_arena("consolidated_edge_nodes", &consolidated_edge_nodes)?;
        namespace.set_arena("consolidated_edge_runs", &consolidated_edge_runs)?;
        namespace.set_arena("consolidated_owner_packets", &consolidated_owner_packets)?;
        namespace.set_arena("consolidated_pcurves", &consolidated_pcurves)?;
        namespace.set_arena(
            "consolidated_vertex_identities",
            &consolidated_vertex_identities,
        )?;
        namespace.set_arena("design_objects", &design_objects)?;
        namespace.set_arena("external_references", &external_references)?;
        namespace.set_arena("catalog_entries", &entries)?;
        namespace.set_arena("object_graphs", &object_graphs)?;
        namespace.set_arena("object_graph_records", &records)?;
        namespace.set_arena("finjpl_segments", &finjpl_segments)?;
        namespace.set_arena("alias_rows", &alias_rows)?;
        namespace.set_arena("preview_images", &preview_images)?;
        namespace.set_arena("value_blocks", &value_blocks)?;
        namespace.set_arena("value_schema_selections", &value_schema_selections)?;
        debug_assert!(CATIA_ARENA_NAMES
            .iter()
            .all(|name| namespace.arenas.contains_key(*name)));
        Ok(())
    }
}

fn value_schema_selections(
    block_id: &str,
    block_byte_offset: u64,
    fields: &[value_block::ValueField],
    catalog: &CatiaCatalog,
) -> Vec<CatiaValueSchemaSelection> {
    let selector_indices = fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let value_block::ValueField::SchemaSelector { ordinal, .. } = field else {
                return None;
            };
            usize::try_from(*ordinal)
                .ok()
                .filter(|ordinal| *ordinal <= catalog.entries.len())
                .map(|_| index)
        })
        .collect::<Vec<_>>();
    selector_indices
        .iter()
        .enumerate()
        .filter_map(|(selector_rank, index)| match &fields[*index] {
            value_block::ValueField::SchemaSelector { ordinal, offset } => {
                let ordinal_index = usize::try_from(*ordinal).ok()?;
                if ordinal_index > catalog.entries.len() {
                    return None;
                }
                let catalog_entry = catalog.entries.get(ordinal_index);
                let entry = catalog_entry.map(|entry| entry.id.clone());
                let value_end = selector_indices
                    .get(selector_rank + 1)
                    .copied()
                    .unwrap_or(fields.len());
                let encoded_value = if entry.is_some() {
                    fields[index + 1..value_end].to_vec()
                } else {
                    Vec::new()
                };
                let byte_offset = block_byte_offset
                    .checked_add(6)?
                    .checked_add(u64::try_from(*offset).ok()?)?;
                Some(CatiaValueSchemaSelection {
                    id: format!("catia:outer:value-selection#{byte_offset:010}"),
                    parent: block_id.to_string(),
                    offset: *offset as u64,
                    ordinal: *ordinal,
                    encoded_value,
                    entry,
                    name: catalog_entry.map(|entry| entry.value.clone()),
                })
            }
            _ => None,
        })
        .collect()
}

impl CatiaValueBlock {
    fn from_parts(
        block: value_block::ValueBlock,
        catalog: &CatiaCatalog,
        object_graph: Option<&CatiaObjectGraph>,
    ) -> Self {
        let id = format!("catia:outer:value-block#{:010}", block.pos);
        let schema_selections =
            value_schema_selections(&id, block.pos as u64, &block.fields, catalog);
        Self {
            id,
            byte_offset: block.pos as u64,
            byte_len: block.total_len as u64,
            declared_len: block.declared_len as u64,
            object_graph: object_graph.map(|graph| graph.id.clone()),
            catalog: catalog.id.clone(),
            payload: block.payload,
            fields: block.fields,
            schema_selections,
        }
    }
}

impl From<object_graph::SurfaceAlias> for CatiaAliasRow {
    fn from(row: object_graph::SurfaceAlias) -> Self {
        Self {
            id: format!("catia:outer:alias-row#{:010}", row.pos),
            byte_offset: row.pos as u64,
            lead: row.lead,
            lead_raw: row.lead_raw,
            tag: row.tag,
            tag_raw: row.tag_raw,
            flag: row.flag,
            f1: row.f1,
            entity_record_ordinal: row.entity_record_ordinal,
            object_graph: None,
            object_record: None,
            design_object: None,
            f2: row.f2,
            f3: row.f3,
            group: row.group,
        }
    }
}

impl From<catalog::Catalog> for CatiaCatalog {
    fn from(catalog: catalog::Catalog) -> Self {
        let id = format!("catia:outer:catalog#{:010}", catalog.pos);
        let entries = catalog
            .entries
            .into_iter()
            .map(|entry| CatiaCatalogEntry {
                id: format!("catia:outer:catalog-entry#{:010}", entry.pos),
                parent: id.clone(),
                ordinal: entry.ordinal,
                byte_offset: entry.pos as u64,
                value: entry.value,
            })
            .collect();
        Self {
            id,
            byte_offset: catalog.pos as u64,
            byte_len: catalog.total_len as u64,
            declared_count: catalog.declared_count,
            entries,
        }
    }
}

impl From<object_graph::ObjectGraph> for CatiaObjectGraph {
    fn from(graph: object_graph::ObjectGraph) -> Self {
        let id = format!("catia:outer:object-graph#{:010}", graph.pos);
        let mut records = graph
            .records
            .into_iter()
            .map(|record| CatiaObjectRecord {
                id: format!("catia:outer:object-record#{:010}", record.pos),
                parent: id.clone(),
                design_object: None,
                ordinal: record.index as u64,
                byte_offset: record.pos as u64,
                byte_len: record.total_len as u64,
                lead: record.lead,
                head: record.head,
                owner_ref: record.owner_ref,
                class_ref: record.class_ref,
                class_name: record.class_name,
                class_entry: None,
                storage_ref: record.storage_ref,
                payload: record.payload,
                subtype: record.subtype,
                references: Vec::new(),
            })
            .collect::<Vec<_>>();
        for record in &mut records {
            record.design_object = record
                .owner_ref
                .map(|owner| design_object_id(graph.pos as u64, owner));
        }
        let record_ids = records
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>();
        let record_design_objects = records
            .iter()
            .map(|record| record.design_object.clone())
            .collect::<Vec<_>>();
        for record in &mut records {
            record.references =
                resolved_payload_references(&record.payload, &record_ids, &record_design_objects);
        }
        Self {
            id,
            byte_offset: graph.pos as u64,
            byte_len: graph.total_len as u64,
            catalog_byte_offset: graph.catalog_pos.map(|pos| pos as u64),
            catalog: None,
            records,
        }
    }
}
