// SPDX-License-Identifier: Apache-2.0
//! Source annotations and retained native records produced during decode.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::annotations::Annotations;
use crate::document::{CadIr, IrVersion};
use crate::hash::digest::Sha256Digest;
use crate::ids::UnknownId;
use crate::native::NativeConvertError;
use crate::provenance::SourceOwner;
use crate::report::decode::DecodeReport;
use crate::unknown::UnknownRecord;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

/// A decode report and source fidelity bound to exact CADIR bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DecodeSidecar {
    /// Exact IR wire version shared with the CADIR document.
    ir_version: IrVersion,
    /// SHA-256 of the exact CADIR bytes this sidecar describes.
    pub ir_sha256: Sha256Digest,
    /// Native decode transfer report.
    pub report: DecodeReport,
    /// Decode-time annotations and retained source records.
    pub fidelity: SourceFidelity,
}

impl DecodeSidecar {
    /// Binds decode metadata to a previously computed CADIR SHA-256 digest.
    pub fn bind_sha256(
        ir_sha256: Sha256Digest,
        report: DecodeReport,
        fidelity: SourceFidelity,
    ) -> Self {
        Self {
            ir_version: IrVersion,
            ir_sha256,
            report,
            fidelity,
        }
    }

    /// Returns whether this sidecar is bound to the supplied CADIR bytes.
    pub fn matches(&self, ir_bytes: &[u8]) -> bool {
        self.ir_sha256 == Sha256Digest::digest(ir_bytes)
    }

    /// Serializes this sidecar as canonical JSON.
    ///
    /// # Errors
    ///
    /// Refuses a non-finite float: the sidecar is an IR document and takes
    /// the same write route as every other.
    pub fn to_canonical_json(
        &self,
    ) -> Result<String, crate::hash::finite_json::CanonicalJsonError> {
        crate::hash::finite_json::to_canonical_json_string(self)
    }

    /// Parses a decode sidecar.
    pub fn from_json(text: &str) -> Result<Self, DecodeSidecarParseError> {
        serde_json::from_str(text).map_err(DecodeSidecarParseError::Json)
    }
}

/// Returns the sidecar path for a CADIR path.
///
/// The sidecar is named after the CADIR file, so a path that names no file has
/// no sidecar path and answers `None`. `Path::file_name` answers `None` for the
/// empty path, for a path that is a root, and for a path whose last component
/// is `..`. The remaining paths all have a name, so `with_file_name` replaces
/// it.
pub fn decode_sidecar_path(path: &Path) -> Option<PathBuf> {
    let file_name = path.file_name()?;
    let stem = match (path.extension(), path.file_stem()) {
        (Some(extension), Some(stem)) if extension == "json" => stem,
        _ if file_name == ".json" => std::ffi::OsStr::new(""),
        _ => file_name,
    };
    let mut sidecar_name = stem.to_os_string();
    sidecar_name.push(".fidelity.json");
    Some(path.with_file_name(sidecar_name))
}

/// Failure parsing a decode sidecar.
#[derive(Debug, thiserror::Error)]
pub enum DecodeSidecarParseError {
    /// Invalid JSON.
    #[error("invalid decode-sidecar JSON: {0}")]
    Json(serde_json::Error),
}

/// The retained image of a source record: its bytes, or the length and digest
/// of bytes that are not retained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "retention", rename_all = "snake_case", deny_unknown_fields)]
pub enum RetainedBytes {
    /// The source bytes themselves. Their length and digest are functions of
    /// them, so neither is stored.
    Inline {
        /// Retained source bytes.
        #[serde(with = "crate::bytes")]
        #[cfg_attr(feature = "schema", schemars(with = "String"))]
        data: Vec<u8>,
    },
    /// Unavailable source bytes, described by their length and digest.
    Digest {
        /// Number of source bytes.
        byte_len: u64,
        /// Lowercase hexadecimal SHA-256 of the source bytes.
        sha256: Sha256Digest,
    },
}

impl RetainedBytes {
    /// Returns the number of source bytes.
    #[must_use]
    pub fn byte_len(&self) -> u64 {
        match self {
            Self::Inline { data } => cadmpeg_core::decode::u64_from_index(data.len()),
            Self::Digest { byte_len, .. } => *byte_len,
        }
    }

    /// Returns the lowercase hexadecimal SHA-256 of the source bytes.
    #[must_use]
    pub fn sha256(&self) -> String {
        match self {
            Self::Inline { data } => crate::hash::sha256_hex(data),
            Self::Digest { sha256, .. } => sha256.as_str().to_owned(),
        }
    }

    /// Returns the source bytes when they are retained.
    #[must_use]
    pub fn data(&self) -> Option<&[u8]> {
        match self {
            Self::Inline { data } => Some(data),
            Self::Digest { .. } => None,
        }
    }
}

/// Source bytes retained for native recovery or replay.
///
/// The record is addressed by its id in
/// [`SourceFidelity::retained_records`], so it carries no id of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "RetainedSourceRecordWire")]
pub struct RetainedSourceRecord {
    /// Source stream containing the record.
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    stream: SourceOwner,
    /// First byte offset in the source stream.
    offset: u64,
    /// Retained image of the source bytes.
    bytes: RetainedBytes,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct RetainedSourceRecordWire {
    /// Source stream containing the record.
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    stream: SourceOwner,
    /// First byte offset in the source stream.
    offset: u64,
    /// Retained image of the source bytes.
    bytes: RetainedBytes,
}

/// A source record's exclusive end offset exceeds the offset domain.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("source record {record}: offset {offset} + {byte_len} bytes exceeds u64")]
pub struct SourceExtentError {
    /// Identity or stream spelling of the affected record.
    pub record: String,
    /// First byte offset.
    pub offset: u64,
    /// Byte length.
    pub byte_len: u64,
}

impl From<SourceExtentError> for cadmpeg_core::CodecError {
    fn from(error: SourceExtentError) -> Self {
        Self::Malformed(error.to_string())
    }
}

fn require_extent(record: &str, offset: u64, byte_len: u64) -> Result<(), SourceExtentError> {
    offset
        .checked_add(byte_len)
        .map(|_| ())
        .ok_or_else(|| SourceExtentError {
            record: record.to_owned(),
            offset,
            byte_len,
        })
}

impl TryFrom<RetainedSourceRecordWire> for RetainedSourceRecord {
    type Error = SourceExtentError;

    fn try_from(wire: RetainedSourceRecordWire) -> Result<Self, Self::Error> {
        Self::from_bytes(wire.stream, wire.offset, wire.bytes)
    }
}

impl RetainedSourceRecord {
    /// Retain a whole source stream. Its start is zero, so its extent fits.
    #[must_use]
    pub fn whole(stream: impl Into<SourceOwner>, data: Vec<u8>) -> Self {
        Self {
            stream: stream.into(),
            offset: 0,
            bytes: RetainedBytes::Inline { data },
        }
    }

    /// Records a source record from a retained image already in hand.
    pub fn from_bytes(
        stream: impl Into<SourceOwner>,
        offset: u64,
        bytes: RetainedBytes,
    ) -> Result<Self, SourceExtentError> {
        let stream = stream.into();
        require_extent(stream.as_str(), offset, bytes.byte_len())?;
        Ok(Self {
            stream,
            offset,
            bytes,
        })
    }

    /// Returns the source stream containing the record.
    #[must_use]
    pub fn stream(&self) -> &str {
        self.stream.as_str()
    }

    /// Assign a containing source owner without changing the admitted byte extent.
    #[must_use]
    pub fn with_owner(mut self, stream: impl Into<SourceOwner>) -> Self {
        self.stream = stream.into();
        self
    }

    /// Returns the first byte offset in the source stream.
    #[must_use]
    pub const fn offset(&self) -> u64 {
        self.offset
    }

    /// Returns the number of source bytes.
    #[must_use]
    pub fn byte_len(&self) -> u64 {
        self.bytes.byte_len()
    }

    /// Returns the exclusive end offset admitted by construction.
    #[must_use]
    pub fn end_offset(&self) -> u64 {
        self.offset + self.bytes.byte_len()
    }

    /// Returns the lowercase hexadecimal SHA-256 of the source bytes.
    #[must_use]
    pub fn sha256(&self) -> String {
        self.bytes.sha256()
    }

    /// Returns the retained bytes when available.
    #[must_use]
    pub fn data(&self) -> Option<&[u8]> {
        self.bytes.data()
    }

    fn from_unknown(
        stream: SourceOwner,
        record: UnknownRecord,
    ) -> Result<(UnknownId, Self), NativeConvertError> {
        let (id, offset, raw, _) = record.into_parts();
        let (id, offset, bytes) = Self::from_unknown_image(stream.as_str(), id, offset, raw)
            .map_err(|error| NativeConvertError::InvalidCollection(error.to_string()))?;
        Ok((
            id,
            Self {
                stream,
                offset,
                bytes,
            },
        ))
    }

    fn from_unknown_image(
        stream: &str,
        id: UnknownId,
        offset: u64,
        raw: crate::unknown::RawRetainedBytes,
    ) -> Result<(UnknownId, u64, RetainedBytes), UnknownImageError<'_>> {
        let bytes = match raw {
            crate::unknown::RawRetainedBytes::Inline { data } => RetainedBytes::Inline { data },
            crate::unknown::RawRetainedBytes::Digest { byte_len, sha256 } => {
                let Ok(sha256) = Sha256Digest::try_from(sha256) else {
                    return Err(UnknownImageError::Digest { id });
                };
                RetainedBytes::Digest { byte_len, sha256 }
            }
        };
        let byte_len = bytes.byte_len();
        if offset.checked_add(byte_len).is_none() {
            return Err(UnknownImageError::Extent {
                id,
                stream,
                offset,
                byte_len,
            });
        }
        Ok((id, offset, bytes))
    }
}

enum UnknownImageError<'stream> {
    Digest {
        id: UnknownId,
    },
    Extent {
        id: UnknownId,
        stream: &'stream str,
        offset: u64,
        byte_len: u64,
    },
}
impl std::fmt::Display for UnknownImageError<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Digest { id } => write!(formatter, "retained record {id}: {}", crate::hash::digest::InvalidSha256Digest),
            Self::Extent { id, stream, offset, byte_len } => write!(formatter, "retained record {id}: source record {stream}: offset {offset} + {byte_len} bytes exceeds u64"),
        }
    }
}

/// Decode-time source annotations and retained native records.
///
/// Retained identities are admitted only through checked insertion.
///
/// ```compile_fail
/// let mut fidelity = cadmpeg_ir::SourceFidelity::default();
/// fidelity.retained_records().clear();
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SourceFidelity {
    /// Sparse source locations and conversion exactness.
    #[serde(default)]
    pub annotations: Annotations,
    /// Native records retained for recovery or replay, keyed by record id.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    retained_records: BTreeMap<UnknownId, RetainedSourceRecord>,
}

impl SourceFidelity {
    /// Fidelity carrying only the given annotations.
    pub fn with_annotations(annotations: Annotations) -> Self {
        Self {
            annotations,
            retained_records: std::collections::BTreeMap::new(),
        }
    }

    /// Consume the source metadata without copying its retained bytes.
    pub fn into_parts(self) -> (Annotations, BTreeMap<UnknownId, RetainedSourceRecord>) {
        (self.annotations, self.retained_records)
    }

    /// Append source metadata after checking both tables for identity collisions.
    /// Failure leaves this source metadata unchanged.
    pub fn append(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        mut other: Self,
    ) -> Result<(), cadmpeg_core::CodecError> {
        if let Some((id, _)) = ctx.find_by(
            &other.retained_records,
            |(id, _)| {
                ctx.contains_key_btree_map(
                    &self.retained_records,
                    *id,
                    "check appended source records",
                )
            },
            "walk appended source records",
        )? {
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!("duplicate retained or native unknown record {id}"),
                "report duplicate source record",
            )?));
        }
        crate::annotations::admit_btree_append(
            ctx,
            &self.retained_records,
            &other.retained_records,
            |id| id.as_str().len(),
            "append source records",
        )?;
        self.annotations
            .append(ctx, other.annotations, "append source provenance")?
            .map_err(|error| {
                match ctx.format_retained(
                    format_args!("{error}"),
                    "report source annotation collision",
                ) {
                    Ok(message) => CodecError::Malformed(message),
                    Err(error) => error,
                }
            })?;
        self.retained_records.append(&mut other.retained_records);
        Ok(())
    }

    /// Finds a retained source record by identifier.
    pub fn retained_record(&self, id: &str) -> Option<&RetainedSourceRecord> {
        self.retained_records.get(id)
    }

    /// Borrow retained records in identity order.
    pub fn retained_records(&self) -> &BTreeMap<UnknownId, RetainedSourceRecord> {
        &self.retained_records
    }

    /// Insert one record without replacing an existing identity.
    pub fn insert_retained_record(
        &mut self,
        id: UnknownId,
        record: RetainedSourceRecord,
    ) -> Result<(), NativeConvertError> {
        match self.retained_records.entry(id) {
            Entry::Vacant(entry) => {
                entry.insert(record);
                Ok(())
            }
            Entry::Occupied(entry) => Err(duplicate_record(entry.key())),
        }
    }

    /// Remove a retained image when its source is replaced or ceases to apply.
    pub fn remove_retained_record(&mut self, id: &str) -> Option<RetainedSourceRecord> {
        self.retained_records.remove(id)
    }

    /// Retains source records without adding them to the product model.
    ///
    /// The records are consumed: their retained bytes move into the sidecar
    /// rather than being copied into it.
    pub fn retain_unknown_records(
        &mut self,
        stream: impl Into<SourceOwner>,
        records: impl IntoIterator<Item = UnknownRecord>,
    ) -> Result<(), NativeConvertError> {
        let stream = stream.into();
        let mut incoming = BTreeMap::new();
        for record in records {
            if self.retained_records.contains_key(record.id()) || incoming.contains_key(record.id())
            {
                return Err(duplicate_record(record.id()));
            }
            let (id, record) = RetainedSourceRecord::from_unknown(stream.clone(), record)?;
            incoming.insert(id, record);
        }
        self.retained_records.extend(incoming);
        Ok(())
    }

    /// Retains an owned record batch under the decode budget.
    /// Admission completes before the retained table changes.
    pub fn retain_unknown_records_for_decode<S>(
        &mut self,
        ctx: &DecodeContext<'_>,
        stream: &str,
        records: S,
    ) -> Result<(), CodecError>
    where
        S: cadmpeg_core::decode::iter_source::IterSource,
        S::Iter: Iterator<Item = UnknownRecord>,
    {
        let mut storage = ctx.reserve_scoped(0, "stage source records")?;
        let mut incoming = BTreeMap::new();
        for record in ctx.admit_iter(records, "retain source record batch")? {
            if ctx.contains_key_btree_map(
                &self.retained_records,
                record.id(),
                "check retained source identity",
            )? || ctx.contains_key_btree_map(
                &incoming,
                record.id(),
                "check staged source identity",
            )? {
                return Err(CodecError::Malformed(ctx.format_retained(
                    format_args!("native collection is invalid: duplicate retained or native unknown record {}", record.id()),
                    "report duplicate source record",
                )?));
            }
            let (id, offset, raw, _) = record.into_parts();
            let (id, offset, bytes) =
                RetainedSourceRecord::from_unknown_image(stream, id, offset, raw).map_err(
                    |error| match ctx.format_retained(
                        format_args!("native collection is invalid: {error}"),
                        "retain source record refusal",
                    ) {
                        Ok(message) => CodecError::Malformed(message),
                        Err(error) => error,
                    },
                )?;
            let stream = if stream.is_empty() {
                SourceOwner::Root
            } else {
                SourceOwner::from(ctx.copy_retained_text(stream, "retain source record owner")?)
            };
            let record = RetainedSourceRecord {
                stream,
                offset,
                bytes,
            };
            storage.with_storage(|| {
                ctx.insert_btree_map(&mut incoming, id, record, "stage source records")
            })?;
        }
        crate::annotations::admit_btree_append(
            ctx,
            &self.retained_records,
            &incoming,
            |id| id.as_str().len(),
            "append retained source records",
        )?;
        if self.retained_records.is_empty() {
            self.retained_records = storage.commit_value(incoming)?;
        } else {
            self.retained_records.append(&mut incoming);
        }
        Ok(())
    }

    /// Stores source bytes and native references within the decode budget.
    /// Both destinations are committed after all admission succeeds.
    pub fn attach_native_unknown_records(
        &mut self,
        ir: &mut CadIr,
        format: &str,
        records: Vec<UnknownRecord>,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        if records.is_empty() {
            return Ok(());
        }
        let mut native_records = Vec::new();
        if let Some(namespace) =
            ctx.get_btree_map(&ir.native.0, format, "find native unknown namespace")?
        {
            if let Some(prior) =
                ctx.get_btree_map(namespace.arenas(), "unknowns", "find native unknown arena")?
            {
                let mut wires =
                    ctx.admit_iter(prior, "copy native unknown records")?
                        .map(|record| {
                            crate::native::read_record::<crate::unknown::NativeUnknownWire>(
                                ctx, "unknowns", record,
                            )
                        });
                loop {
                    let mut storage = ctx.reserve_scoped(0, "read native unknown product")?;
                    let Some(product) = storage.with_storage(|| {
                        Ok::<_, CodecError>(wires.next().map(|record| {
                            crate::NativeUnknownRecord::from_native_for_decode(
                                ctx,
                                record,
                                "unknowns",
                                "read native unknown product",
                            )
                        }))
                    })?
                    else {
                        break;
                    };
                    let product = match product {
                        Ok(product) => product,
                        Err(CodecError::Malformed(message)) => {
                            ctx.charge_retained(
                                u64_from_index(message.len()),
                                "native unknown product refusal",
                            )?;
                            return Err(CodecError::Malformed(message));
                        }
                        Err(error) => return Err(error),
                    };
                    let id = product.id;
                    ctx.charge_retained(
                        u64_from_index(id.as_str().len()),
                        "native unknown arena identity",
                    )?;
                    let mut links = Vec::new();
                    for identity in ctx.admit_iter(product.links, "read native unknown links")? {
                        ctx.charge_retained(
                            u64_from_index(identity.as_str().len()),
                            "native unknown arena link text",
                        )?;
                        ctx.push_vec(
                            &mut links,
                            serde_json::Value::String(identity.into_string()),
                            "native unknown arena links",
                        )?;
                    }
                    let mut fields = serde_json::Map::new();
                    if !links.is_empty() {
                        ctx.charge_collection_items(1, "native unknown arena link field")?;
                        let name = ctx
                            .copy_retained_text("links", "native unknown arena link field name")?;
                        fields.insert(name, serde_json::Value::Array(links));
                    }
                    ctx.push_vec(
                        &mut native_records,
                        crate::native::NativeRecord::new(id.into(), fields)?,
                        "native unknown arena records",
                    )?;
                    drop(storage);
                }
            }
        }
        let existing_ids =
            ctx.with_scoped_storage("native unknown existing identities", || {
                let mut existing_ids = BTreeSet::new();
                for (_, namespace) in ctx.admit_iter(&ir.native.0, "native unknown namespaces")? {
                    if let Some(records) = ctx.get_btree_map(
                        namespace.arenas(),
                        "unknowns",
                        "find existing native unknown arena",
                    )? {
                        for record in ctx.admit_iter(records, "native unknown identity scan")? {
                            ctx.insert_btree_set(
                                &mut existing_ids,
                                record.id(),
                                "native unknown existing identities",
                            )?;
                        }
                    }
                }
                Ok::<_, CodecError>(existing_ids)
            })?;
        let mut storage = ctx.reserve_scoped(0, "native unknown retained index")?;
        let mut retained = BTreeMap::new();
        for record in ctx.admit_iter(records, "native unknown incoming records")? {
            if ctx.contains_key_btree_map(
                &self.retained_records,
                record.id(),
                "native unknown retained lookup",
            )? || ctx.contains_btree_set(
                &existing_ids.0,
                record.id().as_str(),
                "native unknown existing lookup",
            )? || ctx.contains_key_btree_map(
                &retained,
                record.id(),
                "native unknown staged lookup",
            )? {
                return Err(CodecError::Malformed(ctx.format_retained(
                    format_args!("native collection is invalid: duplicate retained or native unknown record {}", record.id()),
                    "native unknown duplicate message",
                )?));
            }
            let stream = match ctx.get_btree_map(
                &self.annotations.provenance,
                record.id().as_str(),
                "native unknown provenance lookup",
            )? {
                Some(provenance) => provenance.stream(),
                None => "",
            };
            let (id, offset, raw, raw_links) = record.into_parts();
            let mut links = Vec::new();
            for text in ctx.admit_iter(raw_links, "native unknown product links")? {
                let identity = crate::ids::Identity::new_for_decode(
                    ctx,
                    text,
                    "native unknown product link text",
                )?
                .map_err(|error| {
                    match ctx
                        .format_retained(format_args!("{error}"), "native unknown link refusal")
                    {
                        Ok(message) => CodecError::Malformed(message),
                        Err(error) => error,
                    }
                })?;
                ctx.push_vec(
                    &mut links,
                    serde_json::Value::String(identity.into_string()),
                    "native unknown arena links",
                )?;
            }
            let mut fields = serde_json::Map::new();
            if !links.is_empty() {
                ctx.charge_collection_items(1, "native unknown arena link field")?;
                let name =
                    ctx.copy_retained_text("links", "native unknown arena link field name")?;
                fields.insert(name, serde_json::Value::Array(links));
            }
            let (id, offset, bytes) =
                RetainedSourceRecord::from_unknown_image(stream, id, offset, raw).map_err(
                    |error| match ctx.format_retained(
                        format_args!("native collection is invalid: {error}"),
                        "native unknown image refusal",
                    ) {
                        Ok(message) => CodecError::Malformed(message),
                        Err(error) => error,
                    },
                )?;
            let stream = if stream.is_empty() {
                SourceOwner::Root
            } else {
                SourceOwner::from(ctx.copy_retained_text(stream, "native unknown source stream")?)
            };
            let product_id = id.try_clone_for_decode(ctx, "native unknown product identity")?;
            ctx.push_vec(
                &mut native_records,
                crate::native::NativeRecord::new(product_id.into(), fields)?,
                "native unknown arena records",
            )?;
            let record = RetainedSourceRecord {
                stream,
                offset,
                bytes,
            };
            storage.with_storage(|| {
                ctx.insert_btree_map(&mut retained, id, record, "native unknown retained index")
            })?;
        }
        ctx.stable_sort_by(
            &mut native_records,
            |value| value.id(),
            Ord::cmp,
            "native unknown arena records sort",
        )?;
        if let Some(pair) = ctx.find_by(
            native_records.windows(2),
            |pair| {
                ctx.equal_bytes(
                    pair[0].id().as_bytes(),
                    pair[1].id().as_bytes(),
                    "native unknown duplicate comparison",
                )
            },
            "native unknown duplicate scan",
        )? {
            return Err(CodecError::Malformed(ctx.format_retained(
                format_args!(
                    "native collection is invalid: duplicate native unknown record {}",
                    pair[0].id()
                ),
                "native unknown duplicate message",
            )?));
        }
        drop(existing_ids);
        crate::annotations::admit_btree_append(
            ctx,
            &self.retained_records,
            &retained,
            |id| id.as_str().len(),
            "append native unknown source records",
        )?;
        if self.retained_records.is_empty() {
            retained = storage.commit_value(retained)?;
        }
        if let Some(namespace) =
            ctx.get_mut_btree_map(&mut ir.native.0, format, "store native unknown namespace")?
        {
            if let Some(arena) = ctx.get_mut_btree_map(
                namespace.arenas_mut(),
                "unknowns",
                "store native unknown arena",
            )? {
                *arena = native_records;
            } else {
                let name = ctx.copy_retained_text("unknowns", "native unknown arena name")?;
                ctx.insert_btree_map(
                    namespace.arenas_mut(),
                    name,
                    native_records,
                    "store native unknown arena",
                )?;
            }
        } else {
            let mut namespace = crate::native::NativeNamespace::default();
            let name = ctx.copy_retained_text("unknowns", "native unknown arena name")?;
            ctx.insert_btree_map(
                namespace.arenas_mut(),
                name,
                native_records,
                "store native unknown arena",
            )?;
            let format = ctx.copy_retained_text(format, "native unknown namespace name")?;
            ctx.insert_btree_map(
                &mut ir.native.0,
                format,
                namespace,
                "store native unknown namespace",
            )?;
        }
        if self.retained_records.is_empty() {
            self.retained_records = retained;
        } else {
            self.retained_records.append(&mut retained);
        }
        Ok(())
    }
}

fn duplicate_record(id: &UnknownId) -> NativeConvertError {
    NativeConvertError::InvalidCollection(format!(
        "duplicate retained or native unknown record {id}"
    ))
}

#[cfg(test)]
mod tests {
    mod admission;
    mod unknown_keys;

    use std::path::{Path, PathBuf};

    use super::{
        decode_sidecar_path, DecodeSidecar, DecodeSidecarParseError, RetainedSourceRecord,
        SourceFidelity,
    };
    use crate::document::CadIr;
    use crate::hash::digest::Sha256Digest;
    use crate::ids::UnknownId;
    use crate::report::decode::DecodeReport;
    use crate::unknown::UnknownRecord;

    fn record(data: &[u8]) -> RetainedSourceRecord {
        RetainedSourceRecord::whole("source", data.to_vec())
    }

    fn id(key: &str) -> UnknownId {
        UnknownId::mint(format!("synthetic:source:record#{key}")).expect("fixture identity")
    }

    fn report() -> DecodeReport {
        DecodeReport::unclassified(
            "test",
            crate::report::decode::DecodeTransfer::full(true),
            std::collections::BTreeMap::default(),
            Vec::new(),
            Vec::new(),
            crate::report::decode::TransferLedger::default(),
        )
    }

    #[test]
    fn retained_records_are_keyed_by_their_id() {
        let mut sidecar = SourceFidelity::default();
        sidecar
            .insert_retained_record(id("b"), record(&[2]))
            .unwrap();
        sidecar
            .insert_retained_record(id("a"), record(&[1]))
            .unwrap();
        assert_eq!(
            sidecar.retained_records.keys().cloned().collect::<Vec<_>>(),
            [id("a"), id("b")]
        );
        assert_eq!(
            sidecar.retained_record(id("a").as_str()),
            Some(&record(&[1]))
        );

        // A record id is a map key, so a document cannot carry two records
        // under one id. `serde_json` hands both to the visitor, so the reader
        // refuses the second by name rather than keeping the last.
        let error = serde_json::from_str::<SourceFidelity>(
            r#"{"retained_records":{
                 "synthetic:source:record#a":{"stream":"source","offset":0,
                      "bytes":{"retention":"inline","data":"AQ=="}},
                 "synthetic:source:record#a":{"stream":"source","offset":0,
                      "bytes":{"retention":"inline","data":"Ag=="}}}}"#,
        )
        .expect_err("a repeated record id states two records under one identity")
        .to_string();
        assert!(
            error.contains("duplicate key synthetic:source:record#a"),
            "{error}"
        );

        let single = serde_json::from_str::<SourceFidelity>(
            r#"{"retained_records":{
                 "synthetic:source:record#a":{"stream":"source","offset":0,
                      "bytes":{"retention":"inline","data":"Ag=="}}}}"#,
        )
        .expect("one record under one id reads");
        assert_eq!(
            single
                .retained_record("synthetic:source:record#a")
                .and_then(RetainedSourceRecord::data),
            Some([2].as_slice())
        );
    }

    #[test]
    fn a_retained_record_derives_its_length_and_digest_from_inline_bytes() {
        let inline = record(&[1, 2, 3]);
        assert_eq!(inline.byte_len(), 3);
        assert_eq!(inline.sha256(), crate::hash::sha256_hex(&[1, 2, 3]));
        assert_eq!(inline.data(), Some([1, 2, 3].as_slice()));

        let wire = serde_json::to_value(&inline).expect("serializes");
        assert_eq!(wire["bytes"]["retention"], "inline");
        assert!(wire["bytes"].get("byte_len").is_none());
        assert!(wire["bytes"].get("sha256").is_none());
        assert_eq!(
            serde_json::from_value::<RetainedSourceRecord>(wire.clone()).expect("round trip"),
            inline
        );

        let mut restated = wire;
        restated["bytes"]["byte_len"] = serde_json::json!(3);
        let error = serde_json::from_value::<RetainedSourceRecord>(restated)
            .unwrap_err()
            .to_string();
        assert!(error.contains("byte_len"), "{error}");
    }

    #[test]
    fn unknown_conversion_keeps_the_retained_bytes_as_the_record_image() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test context");
        for attach in [false, true] {
            let unknown: UnknownRecord = serde_json::from_value(serde_json::json!({
                "id": "synthetic:model:unknown#0",
                "offset": 7,
                "retention": {"retention": "inline", "data": "AQID"}
            }))
            .unwrap();
            let mut fidelity = SourceFidelity::default();
            if attach {
                fidelity
                    .attach_native_unknown_records(
                        &mut CadIr::empty(),
                        "synthetic",
                        [unknown].into(),
                        &ctx,
                    )
                    .unwrap();
            } else {
                fidelity
                    .retain_unknown_records("source", [unknown])
                    .unwrap();
            }
            let retained = fidelity
                .retained_record("synthetic:model:unknown#0")
                .expect("record is keyed by its id");
            assert_eq!(retained.byte_len(), 3);
            assert_eq!(retained.sha256(), crate::hash::sha256_hex(&[1, 2, 3]));
            assert_eq!(retained.data(), Some([1, 2, 3].as_slice()));
        }
    }

    /// `serde_json` hands every object key to the visitor, duplicates
    /// included, and a derived map keeps the last. An identity-keyed map that
    /// a document states twice is refused by name, not silently reduced.
    #[test]
    fn a_restated_record_id_is_refused_by_name() {
        let record = |stream: &str| {
            serde_json::json!({
                "stream": stream,
                "offset": 0,
                "bytes": {
                    "retention": "digest",
                    "byte_len": 3,
                    "sha256": crate::hash::sha256_hex(&[1, 2, 3])
                }
            })
        };
        let one: SourceFidelity = serde_json::from_value(serde_json::json!({
            "retained_records": {"synthetic:source:record#a": record("s")}
        }))
        .expect("one record reads");
        assert_eq!(one.retained_records.len(), 1);

        let text = format!(
            r#"{{"retained_records":{{"synthetic:source:record#a":{},"synthetic:source:record#a":{}}}}}"#,
            record("s"),
            record("t")
        );
        let text = text.as_str();
        let error = serde_json::from_str::<SourceFidelity>(text)
            .expect_err("a restated record id states two records under one identity")
            .to_string();
        assert!(
            error.contains("duplicate key synthetic:source:record#a"),
            "{error}"
        );
    }

    /// The sidecar is an IR document and takes the one finite write route.
    ///
    /// No type reachable from a `DecodeSidecar` carries an `f64` today, so the
    /// sidecar itself cannot be built holding a non-finite float. What the test
    /// can state is both halves that make the claim: the sidecar's text is what
    /// an independent writer produces for the same value, and the route the
    /// method delegates to refuses a non-finite float in a struct field.
    #[derive(serde::Serialize)]
    struct FloatBearing {
        value: f64,
    }

    #[test]
    fn the_sidecar_writes_through_the_finite_route() {
        let sidecar = DecodeSidecar::bind_sha256(
            crate::hash::digest::Sha256Digest::digest(b"cad-ir"),
            report(),
            SourceFidelity::default(),
        );
        assert_eq!(
            sidecar.to_canonical_json().expect("the sidecar writes"),
            serde_json::to_string_pretty(&sidecar).expect("an independent writer writes it too")
        );

        let refused =
            crate::hash::finite_json::to_canonical_json_string(&FloatBearing { value: f64::NAN });
        assert!(
            refused.is_err(),
            "the route the sidecar delegates to refuses a non-finite float"
        );
        assert!(
            crate::hash::finite_json::to_canonical_json_string(&FloatBearing { value: 1.0 })
                .is_ok()
        );
    }

    #[test]
    fn decode_sidecar_binds_exact_ir_bytes() {
        let sidecar = DecodeSidecar::bind_sha256(
            crate::hash::digest::Sha256Digest::digest(b"cad-ir"),
            report(),
            SourceFidelity::default(),
        );
        assert!(sidecar.matches(b"cad-ir"));
        assert!(!sidecar.matches(b"changed"));

        let digest_sidecar = DecodeSidecar::bind_sha256(
            Sha256Digest::digest(b"cad-ir"),
            sidecar.report.clone(),
            sidecar.fidelity.clone(),
        );
        assert_eq!(digest_sidecar, sidecar);

        let json = sidecar.to_canonical_json().expect("serialize sidecar");
        assert_eq!(DecodeSidecar::from_json(&json).unwrap(), sidecar);
    }

    #[test]
    fn cadir_and_fidelity_sidecar_round_trip_preserves_annotation_streams() {
        let ir = CadIr::empty();
        let ir_json = ir.to_canonical_json().expect("serialize CADIR");
        assert_eq!(CadIr::from_json(&ir_json).expect("parse CADIR"), ir);

        let mut builder = crate::AnnotationBuilder::new();
        let stream = crate::annotations::StreamHandle::new(
            &cadmpeg_test_support::service_decode_context(),
            crate::stream_name!(" \t"),
            "fixture stream handle",
        )
        .unwrap();
        builder
            .note(
                &cadmpeg_test_support::service_decode_context(),
                "synthetic:point#0",
                &stream,
                17,
                Some("point"),
            )
            .unwrap();
        let sidecar = DecodeSidecar::bind_sha256(
            crate::hash::digest::Sha256Digest::digest(ir_json.as_bytes()),
            report(),
            SourceFidelity::with_annotations(builder.build()),
        );

        let sidecar_json = sidecar.to_canonical_json().expect("serialize sidecar");
        let parsed = DecodeSidecar::from_json(&sidecar_json).expect("parse sidecar");
        assert_eq!(parsed, sidecar);
        assert_eq!(
            parsed.fidelity.annotations.provenance["synthetic:point#0"].stream(),
            " \t"
        );
    }

    #[test]
    fn decode_sidecar_states_its_report_identity_once() {
        let sidecar = DecodeSidecar::bind_sha256(
            crate::hash::digest::Sha256Digest::digest(b"cad-ir"),
            report(),
            SourceFidelity::default(),
        );
        let json = sidecar.to_canonical_json().expect("serialize sidecar");
        assert!(
            json.contains("\"classification\": \"unclassified\"")
                && json.contains("\"format\": \"test\""),
            "{json}"
        );

        let parsed = DecodeSidecar::from_json(&json).expect("the sidecar round-trips");
        assert!(parsed.report.dialects().is_none());
        assert_eq!(
            serde_json::from_str::<DecodeSidecar>(&json)
                .expect("direct sidecar deserialization uses the same policy"),
            parsed
        );
    }

    #[test]
    fn decode_sidecar_reports_a_missing_report_at_the_outer_boundary() {
        let sidecar = DecodeSidecar::bind_sha256(
            crate::hash::digest::Sha256Digest::digest(b"cad-ir"),
            report(),
            SourceFidelity::default(),
        );
        let mut value = serde_json::to_value(sidecar).unwrap();
        value.as_object_mut().unwrap().remove("report");

        let error = DecodeSidecar::from_json(&value.to_string())
            .expect_err("a sidecar requires its report object");
        assert!(
            matches!(error, DecodeSidecarParseError::Json(ref error) if error.to_string().contains("missing field `report`")),
            "{error}"
        );
    }

    #[test]
    fn a_sidecar_record_cannot_restate_the_length_of_its_inline_bytes() {
        let mut fidelity = SourceFidelity::default();
        fidelity
            .insert_retained_record(id("record"), record(b"payload"))
            .unwrap();
        let sidecar = DecodeSidecar::bind_sha256(
            crate::hash::digest::Sha256Digest::digest(b"cad-ir"),
            report(),
            fidelity,
        );
        let mut value = serde_json::to_value(sidecar).unwrap();
        value["fidelity"]["retained_records"]["synthetic:source:record#record"]["bytes"]
            ["byte_len"] = 1.into();
        let json = value.to_string();

        let error = serde_json::from_str::<DecodeSidecar>(&json)
            .expect_err("an inline record has no length field");
        assert!(error.to_string().contains("byte_len"), "{error}");
    }

    #[test]
    fn decode_sidecar_path_replaces_only_a_trailing_json_suffix() {
        assert_eq!(
            decode_sidecar_path(Path::new("part.cadir.json")),
            Some(PathBuf::from("part.cadir.fidelity.json"))
        );
        assert_eq!(
            decode_sidecar_path(Path::new("part.cadir")),
            Some(PathBuf::from("part.cadir.fidelity.json"))
        );
        for (source, sidecar) in [
            ("dir/.json", "dir/.fidelity.json"),
            ("dir/part.JSON", "dir/part.JSON.fidelity.json"),
            ("dir/part.json.cadir", "dir/part.json.cadir.fidelity.json"),
        ] {
            assert_eq!(
                decode_sidecar_path(Path::new(source)),
                Some(PathBuf::from(sidecar))
            );
        }
    }

    #[test]
    fn decode_sidecar_path_is_absent_for_a_path_that_names_no_file() {
        for source in ["..", "/", "", "dir/..", "/.."] {
            assert_eq!(
                decode_sidecar_path(Path::new(source)),
                None,
                "{source} names no file"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn decode_sidecar_path_preserves_distinct_non_utf8_filenames() {
        use std::os::unix::ffi::OsStringExt;

        for byte in [0xfe, 0xff] {
            for suffix in [b".json".as_slice(), b".cadir".as_slice()] {
                let mut name = vec![byte];
                name.extend_from_slice(suffix);
                let path = Path::new("directory").join(std::ffi::OsString::from_vec(name));
                let mut expected = vec![byte];
                if suffix != b".json" {
                    expected.extend_from_slice(suffix);
                }
                expected.extend_from_slice(b".fidelity.json");
                assert_eq!(
                    decode_sidecar_path(&path),
                    Some(Path::new("directory").join(std::ffi::OsString::from_vec(expected))),
                );
            }
        }
    }
}
