// SPDX-License-Identifier: Apache-2.0
//! CATIA-native ownership and design records retained outside the neutral model.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::catalog;
use crate::object_graph::{self, HeadToken, ObjectPayload, PayloadSubtype};

/// Current schema version for the CATIA native namespace.
pub const CATIA_NATIVE_VERSION: u32 = 1;

const CATIA_ARENA_NAMES: &[&str] = &[
    "catalog_entries",
    "catalogs",
    "object_graph_records",
    "object_graphs",
];

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
    /// First head reference, identifying the owner ordinal.
    pub owner_ref: Option<u32>,
    /// Second head reference, identifying the per-file class ordinal.
    pub class_ref: Option<u32>,
    /// Third head reference, selecting class-specific storage.
    pub storage_ref: Option<u32>,
    /// Typed nested payload.
    pub payload: ObjectPayload,
    /// Structural payload classification.
    pub subtype: PayloadSubtype,
}

/// CATIA-native records retained outside the format-neutral model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CatiaNative {
    /// Schema version this namespace was written under.
    pub version: u32,
    /// Framed source-schema name catalogs.
    #[serde(default)]
    pub catalogs: Vec<CatiaCatalog>,
    /// Outer ownership graphs.
    #[serde(default)]
    pub object_graphs: Vec<CatiaObjectGraph>,
}

impl Default for CatiaNative {
    fn default() -> Self {
        Self {
            version: CATIA_NATIVE_VERSION,
            catalogs: Vec::new(),
            object_graphs: Vec::new(),
        }
    }
}

impl CatiaNative {
    /// Decode CATIA-native records directly from the complete file image.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Self {
        let catalogs = catalog::parse(bytes)
            .into_iter()
            .map(CatiaCatalog::from)
            .collect();
        let object_graphs = object_graph::parse(bytes)
            .into_iter()
            .map(CatiaObjectGraph::from)
            .collect();
        Self {
            version: CATIA_NATIVE_VERSION,
            catalogs,
            object_graphs,
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
        Ok(Self {
            version: namespace.version,
            catalogs,
            object_graphs: graphs,
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
        namespace.set_arena("catalog_entries", &entries)?;
        namespace.set_arena("object_graphs", &graphs)?;
        namespace.set_arena("object_graph_records", &records)?;
        debug_assert!(CATIA_ARENA_NAMES
            .iter()
            .all(|name| namespace.arenas.contains_key(*name)));
        Ok(())
    }
}

impl From<catalog::Catalog> for CatiaCatalog {
    fn from(catalog: catalog::Catalog) -> Self {
        let id = format!("catia:catalog#{:010}", catalog.pos);
        let entries = catalog
            .entries
            .into_iter()
            .map(|entry| CatiaCatalogEntry {
                id: format!("catia:catalog-entry#{:010}", entry.pos),
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
        let id = format!("catia:object-graph#{:010}", graph.pos);
        let records = graph
            .records
            .into_iter()
            .map(|record| CatiaObjectRecord {
                id: format!("catia:object-record#{:010}", record.pos),
                parent: id.clone(),
                ordinal: record.index as u64,
                byte_offset: record.pos as u64,
                byte_len: record.total_len as u64,
                lead: record.lead,
                head: record.head,
                owner_ref: record.owner_ref,
                class_ref: record.class_ref,
                storage_ref: record.storage_ref,
                payload: record.payload,
                subtype: record.subtype,
            })
            .collect();
        Self {
            id,
            byte_offset: graph.pos as u64,
            byte_len: graph.total_len as u64,
            records,
        }
    }
}
