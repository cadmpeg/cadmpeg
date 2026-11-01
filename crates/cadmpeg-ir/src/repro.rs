// SPDX-License-Identifier: Apache-2.0
//! Closed CADIR fragments and stable finding signatures.

use crate::{report::check::Finding, CadIr};
use cadmpeg_core::CodecError;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

pub mod signature;
pub use signature::{NumberBand, Signature, Stage};
mod privacy;
#[doc(hidden)]
pub mod selection;

/// The document consumed by fragment mappers.
pub type Document = CadIr;

/// Caller context retained without interpretation.
#[derive(Debug, Clone)]
pub struct CutOptions {
    /// Application data for the consented bundle.
    pub context: Map<String, Value>,
    /// Maximum arena rows admitted before projection.
    pub max_rows: usize,
    /// Maximum compact document bytes admitted before projection.
    pub max_serialized_bytes: usize,
}

impl Default for CutOptions {
    fn default() -> Self {
        Self {
            context: Map::new(),
            max_rows: 1_000_000,
            max_serialized_bytes: 64 * 1024 * 1024,
        }
    }
}

/// One retained arena's size and purpose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ManifestEntry {
    /// Dotted arena address.
    pub arena: String,
    /// Retained rows.
    pub rows: usize,
    /// Compact JSON array size in bytes.
    pub serialized_bytes: usize,
    /// One-line arena description.
    pub description: String,
}

/// A fragment and its reproduction contract.
#[derive(Debug, Clone, PartialEq)]
pub struct Bundle {
    /// A normal CADIR document with its fragment flag set.
    pub document: Document,
    /// Finding identity before reduction.
    pub signature: Signature,
    /// Stripped root identities.
    pub roots: Vec<String>,
    /// Opaque caller data.
    pub context: Map<String, Value>,
    /// Consent-screen inventory.
    pub manifest: Vec<ManifestEntry>,
    /// Reason this bounded cut requires more context to reproduce.
    pub incomplete_reason: Option<String>,
}

/// Reproduction metadata stored in a normal CADIR document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BundleMetadata {
    /// Normalized finding contract.
    pub signature: Signature,
    /// Stripped root identities.
    pub roots: Vec<String>,
    /// Opaque application data.
    pub context: Map<String, Value>,
    /// Consent-screen inventory.
    pub manifest: Vec<ManifestEntry>,
    /// Reason this cut exceeds its resource bound or requires global context.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_incomplete_reason"
    )]
    pub incomplete_reason: Option<String>,
}

impl Serialize for Bundle {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut document = self.document.clone();
        document.repro = Some(Box::new(BundleMetadata {
            signature: self.signature.clone(),
            roots: self.roots.clone(),
            context: self.context.clone(),
            manifest: self.manifest.clone(),
            incomplete_reason: self.incomplete_reason.clone(),
        }));
        document.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Bundle {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut document = crate::document::CadIr::deserialize(deserializer)?;
        if !document.fragment {
            return Err(serde::de::Error::custom("bundle requires fragment=true"));
        }
        let metadata = document
            .repro
            .take()
            .ok_or_else(|| serde::de::Error::custom("bundle requires repro metadata"))?;
        Ok(Self {
            document,
            signature: metadata.signature,
            roots: metadata.roots,
            context: metadata.context,
            manifest: metadata.manifest,
            incomplete_reason: metadata.incomplete_reason,
        })
    }
}

/// A row address within a document projection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Row {
    /// Model or native arena JSON pointer.
    pub arena: String,
    /// Position within the array.
    pub index: usize,
}

/// Index every model and native arena row.
pub fn rows(value: &Value) -> Vec<Row> {
    let mut rows = Vec::new();
    if let Some(model) = value["model"].as_object() {
        for (name, arena) in model {
            add_rows(&mut rows, &format!("/model/{name}"), arena);
        }
    }
    if let Some(native) = value["native"].as_object() {
        for (codec, namespace) in native {
            if let Some(namespace) = namespace.as_object() {
                for (name, arena) in namespace {
                    add_rows(&mut rows, &format!("/native/{codec}/{name}"), arena);
                }
            }
        }
    }
    rows
}

fn add_rows(rows: &mut Vec<Row>, arena: &str, value: &Value) {
    if let Some(values) = value.as_array() {
        rows.extend((0..values.len()).map(|index| Row {
            arena: arena.to_owned(),
            index,
        }));
    }
}

/// Read a projected row.
pub fn record<'a>(value: &'a Value, row: &Row) -> &'a Value {
    &value.pointer(&row.arena).unwrap_or(&Value::Null)[row.index]
}

/// Shared graph edges indexed by row position.
pub fn edges(value: &Value, rows: &[Row]) -> Vec<Vec<usize>> {
    let mut ids: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        if let Some(id) = record(value, row)["id"].as_str() {
            ids.entry(id).or_default().push(index);
        }
    }
    rows.iter()
        .map(|row| {
            let mut targets = Vec::new();
            crate::reference_walk::visit_edges(
                record(value, row),
                "",
                true,
                &|id| ids.get(id).cloned().unwrap_or_default(),
                &mut |_, target| {
                    targets.push(target);
                    true
                },
            );
            targets
        })
        .collect()
}

/// Retain a set of rows while preserving arena order.
pub fn retain(value: &mut Value, rows: &[Row], keep: &BTreeSet<usize>) {
    let arenas: BTreeSet<&str> = rows.iter().map(|row| row.arena.as_str()).collect();
    for arena in arenas {
        let retained: BTreeSet<usize> = keep
            .iter()
            .filter_map(|index| {
                let row = &rows[*index];
                (row.arena == arena).then_some(row.index)
            })
            .collect();
        if let Some(values) = value.pointer_mut(arena).and_then(Value::as_array_mut) {
            let mut index = 0;
            values.retain(|_| {
                let keep = retained.contains(&index);
                index += 1;
                keep
            });
        }
    }
}

/// Measure a bounded serialized document before validation.
pub fn projection_size(document: &Document, options: &CutOptions) -> Result<usize, CodecError> {
    Ok(project(document, options)?.bytes.len())
}

fn project(document: &Document, options: &CutOptions) -> Result<LimitedProjection, CodecError> {
    let mut count = document.model.entity_count();
    for namespace in document.native.0.values() {
        for arena in namespace.arenas().values() {
            count = count
                .checked_add(arena.len())
                .ok_or_else(|| CodecError::InvalidInput("fragment row count overflow".into()))?;
        }
    }
    if count > options.max_rows {
        return Err(CodecError::InvalidInput(
            "fragment row limit exceeded".into(),
        ));
    }
    let mut projection = LimitedProjection {
        bytes: Vec::new(),
        limit: options.max_serialized_bytes,
    };
    serde_json::to_writer(&mut projection, document).map_err(CodecError::malformed)?;
    Ok(projection)
}

/// Build the reference closure and required topology context of a finding.
pub fn cut(
    document: &Document,
    finding: &Finding,
    options: &CutOptions,
) -> Result<Bundle, CodecError> {
    let Some(selected) = selection::select(document, finding, options)? else {
        return Ok(Bundle::incomplete(
            document,
            finding,
            options,
            "required closure exceeds the cut resource bound",
        ));
    };
    cut_with_native(
        &selected,
        finding,
        options,
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
}

/// Apply codec-owned string classification to a closed fragment.
#[doc(hidden)]
pub fn cut_with_native(
    document: &Document,
    finding: &Finding,
    options: &CutOptions,
    native_enum_fields: &crate::native::privacy::EnumFields,
    native_text_replacements: &BTreeMap<String, String>,
) -> Result<Bundle, CodecError> {
    let projection = project(document, options)?;
    let mut value: Value =
        serde_json::from_slice(&projection.bytes).map_err(CodecError::malformed)?;
    let roots: Vec<String> = finding
        .entity
        .iter()
        .chain(&finding.related)
        .cloned()
        .collect();
    if roots.is_empty() {
        return Ok(Bundle::incomplete(
            document,
            finding,
            options,
            "finding requires complete-document context",
        ));
    }
    value["fragment"] = Value::Bool(true);
    if let Some(object) = value.as_object_mut() {
        object.remove("repro");
    }
    let codec = document
        .source
        .as_ref()
        .map_or("cadir", |source| source.format());
    let signature = Signature::new(finding, codec, Stage::Check);
    let (value, roots) = privacy::strip(
        value,
        roots,
        native_enum_fields,
        native_text_replacements,
        document
            .source
            .as_ref()
            .map(crate::document::SourceMeta::format),
    )?;
    let mut document: Document = serde_json::from_value(value).map_err(CodecError::malformed)?;
    if finding.check != crate::report::check::Check::ArenaOrder {
        document.finalize();
    }
    let mut bundle = Bundle {
        document,
        signature,
        roots,
        context: options.context.clone(),
        manifest: Vec::new(),
        incomplete_reason: None,
    };
    bundle.refresh_manifest()?;
    Ok(bundle)
}

impl Bundle {
    /// Return a bounded document with an explicit incomplete-cut reason.
    pub fn incomplete(
        document: &Document,
        finding: &Finding,
        options: &CutOptions,
        reason: &str,
    ) -> Self {
        let mut fragment = Document::empty();
        fragment.fragment = true;
        fragment.tolerances = document.tolerances;
        fragment.source = document
            .source
            .as_ref()
            .map(crate::document::SourceMeta::repro_header);
        let codec = document
            .source
            .as_ref()
            .map_or("cadir", |source| source.format());
        Self {
            document: fragment,
            signature: Signature::new(finding, codec, Stage::Check),
            roots: Vec::new(),
            context: options.context.clone(),
            manifest: Vec::new(),
            incomplete_reason: Some(reason.into()),
        }
    }

    /// Recalculate the inventory after row removal.
    pub fn refresh_manifest(&mut self) -> Result<(), CodecError> {
        let value = serde_json::to_value(&self.document).map_err(CodecError::malformed)?;
        let arenas: BTreeSet<String> = rows(&value).into_iter().map(|row| row.arena).collect();
        self.manifest = arenas
            .into_iter()
            .map(|arena| {
                let values = value.pointer(&arena).unwrap_or(&Value::Null);
                Ok(ManifestEntry {
                    description: format!("{} records", arena.rsplit('/').next().unwrap_or("arena")),
                    arena: arena.trim_start_matches('/').replace('/', "."),
                    rows: values.as_array().map_or(0, Vec::len),
                    serialized_bytes: serde_json::to_vec(values)
                        .map_err(CodecError::malformed)?
                        .len(),
                })
            })
            .collect::<Result<_, CodecError>>()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

struct LimitedProjection {
    bytes: Vec<u8>,
    limit: usize,
}
impl std::io::Write for LimitedProjection {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let total = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| std::io::Error::other("fragment byte count overflow"))?;
        if total > self.limit {
            return Err(std::io::Error::other("fragment byte limit exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

cadmpeg_core::named_optional_field!(deserialize_incomplete_reason, String, "incomplete_reason");
