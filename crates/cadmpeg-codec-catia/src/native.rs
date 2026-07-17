// SPDX-License-Identifier: Apache-2.0
//! CATIA-native ownership and design records retained outside the neutral model.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::catalog;
use crate::container;
use crate::object_graph::{
    self, AliasLead, HeadToken, ListItem, ObjectPayload, PayloadField, PayloadSubtype,
};
use crate::value_block;

/// Current schema version for the CATIA native namespace.
pub const CATIA_NATIVE_VERSION: u32 = 26;

const CATIA_ARENA_NAMES: &[&str] = &[
    "alias_rows",
    "catalog_entries",
    "catalogs",
    "design_objects",
    "external_references",
    "finjpl_segments",
    "object_graph_records",
    "object_graphs",
    "preview_images",
    "value_blocks",
];

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
    /// First trailing fixed-width field.
    pub f2: u32,
    /// Second trailing fixed-width field.
    pub f3: u32,
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
    /// Byte offset within the value payload.
    pub offset: u64,
    /// Stored zero-based ordinal or terminal absent-schema sentinel.
    pub ordinal: u32,
    /// Selected catalog entry; absent for the terminal sentinel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// Encoded value token immediately following an in-range selector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<value_block::ValueField>,
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
}

/// One serialized design object formed by a shared `7C09` owner ordinal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaDesignObject {
    /// Globally unique design-object identity.
    pub id: String,
    /// Containing [`CatiaObjectGraph`] identity.
    pub parent: String,
    /// One-based owner ordinal stored by every field record.
    pub owner_ordinal: u32,
    /// Record selected by `owner_ordinal` when it lies inside the graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_record: Option<String>,
    /// Resolved class of the selected owner record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_class: Option<String>,
    /// Class-specific storage selector of the selected owner record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_storage_ref: Option<u32>,
    /// Field records carrying this owner ordinal, in serialized order.
    pub fields: Vec<String>,
    /// Distinct resolved field classes, in first field order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub field_classes: Vec<String>,
    /// Referenced design objects, in first field-reference order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
}

fn design_objects(graphs: &[CatiaObjectGraph]) -> Vec<CatiaDesignObject> {
    graphs
        .iter()
        .flat_map(|graph| {
            let mut fields = BTreeMap::<u32, Vec<&CatiaObjectRecord>>::new();
            for record in &graph.records {
                if let Some(owner) = record.owner_ref {
                    fields.entry(owner).or_default().push(record);
                }
            }
            let owners = fields.keys().copied().collect::<Vec<_>>();
            fields.into_iter().map(move |(owner_ordinal, records)| {
                let owner_record = object_record_index(owner_ordinal, graph.records.len())
                    .and_then(|index| graph.records.get(index));
                let mut dependency_owners = Vec::new();
                for reference in records
                    .iter()
                    .flat_map(|record| payload_references(&record.payload))
                {
                    let target_owner = object_record_index(reference, graph.records.len())
                        .and_then(|index| graph.records.get(index))
                        .and_then(|record| record.owner_ref);
                    if let Some(target_owner) = target_owner.filter(|target| {
                        *target != owner_ordinal
                            && owners.contains(target)
                            && !dependency_owners.contains(target)
                    }) {
                        dependency_owners.push(target_owner);
                    }
                }
                CatiaDesignObject {
                    id: design_object_id(graph.byte_offset, owner_ordinal),
                    parent: graph.id.clone(),
                    owner_ordinal,
                    owner_record: owner_record.map(|record| record.id.clone()),
                    owner_class: owner_record.and_then(|record| record.class_name.clone()),
                    owner_storage_ref: owner_record.and_then(|record| record.storage_ref),
                    fields: records.iter().map(|record| record.id.clone()).collect(),
                    field_classes: records
                        .iter()
                        .filter_map(|record| record.class_name.clone())
                        .fold(Vec::new(), |mut classes, class| {
                            if !classes.contains(&class) {
                                classes.push(class);
                            }
                            classes
                        }),
                    dependencies: dependency_owners
                        .into_iter()
                        .map(|owner| design_object_id(graph.byte_offset, owner))
                        .collect(),
                }
            })
        })
        .collect()
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

/// CATIA-native records retained outside the format-neutral model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaNative {
    /// Schema version this namespace was written under.
    pub version: u32,
    /// Exact outer alias-row cores in source order.
    #[serde(default)]
    pub alias_rows: Vec<CatiaAliasRow>,
    /// Framed source-schema name catalogs.
    #[serde(default)]
    pub catalogs: Vec<CatiaCatalog>,
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
            design_objects: Vec::new(),
            external_references: Vec::new(),
            finjpl_segments: Vec::new(),
            object_graphs: Vec::new(),
            preview_images: Vec::new(),
            value_blocks: Vec::new(),
        }
    }
}

impl CatiaNative {
    /// Decode CATIA-native records directly from the complete file image.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Self {
        let mut catalogs: Vec<CatiaCatalog> = catalog::parse(bytes)
            .into_iter()
            .map(CatiaCatalog::from)
            .collect();
        let mut alias_rows = object_graph::surface_aliases(bytes)
            .into_iter()
            .map(CatiaAliasRow::from)
            .collect::<Vec<_>>();
        let object_graphs: Vec<CatiaObjectGraph> = object_graph::parse_all(bytes)
            .into_iter()
            .map(CatiaObjectGraph::from)
            .collect();
        let parsed_value_blocks = value_block::parse(bytes);
        alias_rows.retain(|row| {
            let row_start = row.byte_offset.saturating_sub(4);
            let row_end = row.byte_offset + 20;
            !object_graphs.iter().any(|graph| {
                row_start < graph.byte_offset + graph.byte_len && row_end > graph.byte_offset
            }) && !parsed_value_blocks.iter().any(|block| {
                row_start < (block.pos + block.total_len) as u64 && row_end > block.pos as u64
            })
        });
        catalogs.retain(|catalog| {
            !object_graphs.iter().any(|graph| {
                catalog.byte_offset >= graph.byte_offset
                    && catalog.byte_offset < graph.byte_offset + graph.byte_len
            }) && !parsed_value_blocks.iter().any(|block| {
                catalog.byte_offset >= block.pos as u64
                    && catalog.byte_offset < (block.pos + block.total_len) as u64
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
        let preview_images = container::preview_images(bytes)
            .into_iter()
            .enumerate()
            .map(|(index, preview)| CatiaPreviewImage {
                id: format!("catia:outer:preview#{index}"),
                byte_offset: preview.range.start as u64,
                byte_len: (preview.range.end - preview.range.start) as u64,
                width: preview.width,
                height: preview.height,
                components: preview.components,
                data: bytes[preview.range].to_vec(),
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
                family: match segment.kind {
                    container::FinjplKind::Storage => "storage",
                    container::FinjplKind::ProjectFlags => "project-flags",
                    container::FinjplKind::Other => "other",
                }
                .to_string(),
                name: segment.name,
                data: bytes[segment.range].to_vec(),
            })
            .collect::<Vec<_>>();
        let external_references = container::external_references(bytes)
            .into_iter()
            .enumerate()
            .filter_map(|(index, reference)| {
                let offset = reference.offset as u64;
                let segment = finjpl_segments.iter().find(|segment| {
                    offset >= segment.byte_offset
                        && offset < segment.byte_offset + segment.byte_len
                        && segment.family == "project-flags"
                })?;
                Some(CatiaExternalReference {
                    id: format!("catia:outer:external-reference#{index}"),
                    byte_offset: offset,
                    target: reference.target,
                    segment: segment.id.clone(),
                })
            })
            .collect();
        Self {
            version: CATIA_NATIVE_VERSION,
            alias_rows,
            catalogs,
            design_objects,
            external_references,
            finjpl_segments,
            object_graphs,
            preview_images,
            value_blocks,
        }
    }

    /// Load the typed CATIA namespace from generic native arenas.
    pub fn load(
        namespace: &cadmpeg_ir::NativeNamespace,
    ) -> Result<Self, cadmpeg_ir::NativeConvertError> {
        let mut catalogs: Vec<CatiaCatalog> = namespace.arena_as("catalogs")?;
        let entries: Vec<CatiaCatalogEntry> = namespace.arena_as("catalog_entries")?;
        for catalog in &mut catalogs {
            catalog.entries = entries
                .iter()
                .filter(|entry| entry.parent == catalog.id)
                .cloned()
                .collect();
            catalog.entries.sort_by_key(|entry| entry.ordinal);
        }
        let mut graphs: Vec<CatiaObjectGraph> = namespace.arena_as("object_graphs")?;
        let records: Vec<CatiaObjectRecord> = namespace.arena_as("object_graph_records")?;
        for graph in &mut graphs {
            graph.records = records
                .iter()
                .filter(|record| record.parent == graph.id)
                .cloned()
                .collect();
            graph.records.sort_by_key(|record| record.ordinal);
        }
        let value_blocks: Vec<CatiaValueBlock> = namespace.arena_as("value_blocks")?;
        let design_objects = if namespace.arenas.contains_key("design_objects") {
            namespace.arena_as("design_objects")?
        } else {
            design_objects(&graphs)
        };
        let finjpl_segments: Vec<CatiaFinjplSegment> =
            if namespace.arenas.contains_key("finjpl_segments") {
                namespace.arena_as("finjpl_segments")?
            } else {
                Vec::new()
            };
        let external_references: Vec<CatiaExternalReference> =
            if namespace.arenas.contains_key("external_references") {
                namespace.arena_as("external_references")?
            } else {
                Vec::new()
            };
        let preview_images: Vec<CatiaPreviewImage> =
            if namespace.arenas.contains_key("preview_images") {
                namespace.arena_as("preview_images")?
            } else {
                Vec::new()
            };
        let alias_rows: Vec<CatiaAliasRow> = namespace.arena_as("alias_rows")?;
        Ok(Self {
            version: namespace.version,
            alias_rows,
            catalogs,
            design_objects,
            external_references,
            finjpl_segments,
            object_graphs: graphs,
            preview_images,
            value_blocks,
        })
    }

    /// Store the typed CATIA namespace into generic native arenas.
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
        namespace.set_arena("catalogs", &catalogs)?;
        namespace.set_arena("design_objects", &self.design_objects)?;
        namespace.set_arena("external_references", &self.external_references)?;
        namespace.set_arena("finjpl_segments", &self.finjpl_segments)?;
        namespace.set_arena("alias_rows", &self.alias_rows)?;
        namespace.set_arena("catalog_entries", &entries)?;
        namespace.set_arena("object_graphs", &graphs)?;
        namespace.set_arena("object_graph_records", &records)?;
        namespace.set_arena("preview_images", &self.preview_images)?;
        namespace.set_arena("value_blocks", &self.value_blocks)?;
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
            version,
            alias_rows,
            mut catalogs,
            design_objects,
            external_references,
            finjpl_segments,
            mut object_graphs,
            preview_images,
            value_blocks,
        } = self;
        let entries = catalogs
            .iter_mut()
            .flat_map(|catalog| std::mem::take(&mut catalog.entries))
            .collect::<Vec<_>>();
        let records = object_graphs
            .iter_mut()
            .flat_map(|graph| std::mem::take(&mut graph.records))
            .collect::<Vec<_>>();

        namespace.version = version;
        namespace.set_arena_owned("catalogs", catalogs)?;
        namespace.set_arena_owned("design_objects", design_objects)?;
        namespace.set_arena_owned("external_references", external_references)?;
        namespace.set_arena_owned("catalog_entries", entries)?;
        namespace.set_arena_owned("object_graphs", object_graphs)?;
        namespace.set_arena_owned("object_graph_records", records)?;
        namespace.set_arena_owned("finjpl_segments", finjpl_segments)?;
        namespace.set_arena_owned("alias_rows", alias_rows)?;
        namespace.set_arena_owned("preview_images", preview_images)?;
        namespace.set_arena_owned("value_blocks", value_blocks)?;
        debug_assert!(CATIA_ARENA_NAMES
            .iter()
            .all(|name| namespace.arenas.contains_key(*name)));
        Ok(())
    }
}

impl CatiaValueBlock {
    fn from_parts(
        block: value_block::ValueBlock,
        catalog: &CatiaCatalog,
        object_graph: Option<&CatiaObjectGraph>,
    ) -> Self {
        let selector_indices = block
            .fields
            .iter()
            .enumerate()
            .filter_map(|(index, field)| {
                matches!(field, value_block::ValueField::SchemaSelector { .. }).then_some(index)
            })
            .collect::<Vec<_>>();
        let schema_selections = selector_indices
            .iter()
            .enumerate()
            .filter_map(|(selector_rank, index)| match &block.fields[*index] {
                value_block::ValueField::SchemaSelector { ordinal, offset } => {
                    let ordinal_index = usize::try_from(*ordinal).ok()?;
                    if ordinal_index > catalog.entries.len() {
                        return None;
                    }
                    let entry = catalog
                        .entries
                        .get(ordinal_index)
                        .map(|entry| entry.id.clone());
                    let value_end = selector_indices
                        .get(selector_rank + 1)
                        .copied()
                        .unwrap_or(block.fields.len());
                    Some(CatiaValueSchemaSelection {
                        offset: *offset as u64,
                        ordinal: *ordinal,
                        value: entry
                            .as_ref()
                            .and_then(|_| block.fields.get(index + 1))
                            .cloned(),
                        encoded_value: if entry.is_some() {
                            block.fields[index + 1..value_end].to_vec()
                        } else {
                            Vec::new()
                        },
                        entry,
                    })
                }
                _ => None,
            })
            .collect();
        Self {
            id: format!("catia:outer:value-block#{:010}", block.pos),
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
            f2: row.f2,
            f3: row.f3,
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
                storage_ref: record.storage_ref,
                payload: record.payload,
                subtype: record.subtype,
                references: Vec::new(),
            })
            .collect::<Vec<_>>();
        let record_ids = records
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>();
        for record in &mut records {
            record.design_object = record
                .owner_ref
                .map(|owner| design_object_id(graph.pos as u64, owner));
            record.references = payload_references(&record.payload)
                .map(|ordinal| CatiaObjectRecordReference {
                    ordinal,
                    target: object_record_index(ordinal, record_ids.len())
                        .and_then(|index| record_ids.get(index))
                        .cloned(),
                })
                .collect();
        }
        Self {
            id,
            byte_offset: graph.pos as u64,
            byte_len: graph.total_len as u64,
            catalog_byte_offset: graph.catalog_pos.map(|pos| pos as u64),
            records,
        }
    }
}
