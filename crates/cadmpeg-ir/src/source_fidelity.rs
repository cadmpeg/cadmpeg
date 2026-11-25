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
use crate::ids::{ProductDefinitionId, UnknownId};
use crate::native::NativeConvertError;
use crate::provenance::SourceOwner;
use crate::report::decode::DecodeReport;
use crate::unknown::UnknownRecord;
use cadmpeg_core::decode::{u64_from_index, ArenaClass, DecodeContext, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;

/// Retained-byte operation for one appended native unknown record.
const ATTACH_UNKNOWN_RECORD: &str = "attach native unknown record";

/// The `sidecar.stale` loss: a reader ignored a decode sidecar that does not
/// describe the CADIR bytes beside it or that it cannot read.
#[must_use]
pub fn sidecar_stale_kind() -> crate::report::loss::LossKind {
    crate::report::loss::NamespacedLossKind::new(
        crate::loss_namespace!("decode"),
        "sidecar.stale",
        crate::report::loss::LossTaxonomy::DecodeDiagnostic,
    )
    .into()
}

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
        crate::bounds::read_json::<Self>(text).map_err(DecodeSidecarParseError::Json)
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, cadmpeg_derive::DeserializeTagged)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
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
    /// Content classes the bytes can hold where the decoder keeps the record
    /// without decoding it.
    #[serde(serialize_with = "cadmpeg_core::distinct_keys::set::serialize")]
    #[cfg_attr(feature = "schema", schemars(with = "Vec<String>"))]
    untyped: BTreeSet<ArenaClass>,
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
    /// Content classes the bytes can hold where the decoder keeps the record
    /// without decoding it.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::set::deserialize")]
    #[cfg_attr(feature = "schema", schemars(with = "Vec<String>"))]
    untyped: BTreeSet<ArenaClass>,
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
        Ok(Self::from_bytes(wire.stream, wire.offset, wire.bytes)?.with_untyped(wire.untyped))
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
            untyped: BTreeSet::new(),
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
            untyped: BTreeSet::new(),
        })
    }

    /// Marks the record as kept without decoding: an unknown record type, a
    /// skipped region, or a given-up part, with the content classes its bytes
    /// can hold. The classes join any already marked.
    #[must_use]
    pub fn with_untyped(mut self, classes: impl IntoIterator<Item = ArenaClass>) -> Self {
        self.untyped.extend(classes);
        self
    }

    /// Returns the content classes the record's bytes can hold where the
    /// decoder kept it without decoding it. Empty for a decoded record.
    #[must_use]
    pub fn untyped(&self) -> &BTreeSet<ArenaClass> {
        &self.untyped
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
        let bytes = match raw {
            crate::unknown::RawRetainedBytes::Inline { data } => RetainedBytes::Inline { data },
            crate::unknown::RawRetainedBytes::Digest { byte_len, sha256 } => {
                RetainedBytes::Digest {
                    byte_len,
                    sha256: Sha256Digest::try_from(sha256).map_err(|error| {
                        NativeConvertError::InvalidCollection(format!(
                            "retained record {id}: {error}"
                        ))
                    })?,
                }
            }
        };
        let retained = Self::from_bytes(stream, offset, bytes).map_err(|error| {
            NativeConvertError::InvalidCollection(format!("retained record {id}: {error}"))
        })?;
        Ok((id, retained))
    }
}

/// The narrower digests a codec's patch path compares to find what changed,
/// under the keys its format specification lists (§3.1, Rule B).
pub type CodecDigests = BTreeMap<NonBlankString, Sha256Digest>;

/// Decode-time source digests, annotations, and retained native records.
///
/// Retained identities are admitted only through checked insertion.
///
/// ```compile_fail
/// use cadmpeg_ir::hash::digest::Sha256Digest;
/// let mut fidelity = cadmpeg_ir::SourceFidelity::new(Sha256Digest::digest(b"source"));
/// fidelity.retained_records().clear();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(with = "SourceFidelityWire"))]
#[serde(try_from = "SourceFidelityWire")]
pub struct SourceFidelity {
    /// SHA-256 of the decode's root file.
    pub decode_digest: Sha256Digest,
    /// SHA-256 of each component file of the decode, keyed by its `File`
    /// product definition. The root definition, whose file `decode_digest`
    /// covers, has no entry.
    pub file_digests: BTreeMap<ProductDefinitionId, Sha256Digest>,
    /// SHA-256 of the canonical JSON of the document the decode returned,
    /// which the decode wrapper takes after it sets `scope` and normalizes;
    /// `None` where that document has no canonical JSON.
    pub content_digest: Option<Sha256Digest>,
    /// The digests by which the decoding codec's patch path finds what
    /// changed; only that codec reads them.
    pub codec_digests: CodecDigests,
    /// Sparse source locations and conversion exactness.
    pub annotations: Annotations,
    /// Native records retained for recovery or replay, keyed by record id.
    retained_records: BTreeMap<UnknownId, RetainedSourceRecord>,
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
struct SourceFidelityWire {
    /// SHA-256 of the decode's root file.
    decode_digest: Sha256Digest,
    /// SHA-256 of each component file, keyed by its `File` definition.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    file_digests: BTreeMap<ProductDefinitionId, Sha256Digest>,
    /// SHA-256 of the canonical JSON of the document the decode returned,
    /// absent where that document has no canonical JSON.
    #[serde(default, deserialize_with = "deserialize_content_digest")]
    content_digest: Option<Sha256Digest>,
    /// The decoding codec's narrower digests, keyed as its format
    /// specification lists.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    codec_digests: CodecDigests,
    /// Union of the retained records' untyped classes.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::set::deserialize")]
    #[cfg_attr(feature = "schema", schemars(with = "Vec<String>"))]
    unrepresented: BTreeSet<ArenaClass>,
    /// Sparse source locations and conversion exactness.
    annotations: Annotations,
    /// Native records retained for recovery or replay, keyed by record id.
    #[serde(deserialize_with = "cadmpeg_core::distinct_keys::btree_map")]
    retained_records: BTreeMap<UnknownId, RetainedSourceRecord>,
}

cadmpeg_core::named_optional_field!(deserialize_content_digest, Sha256Digest, "content_digest");

impl TryFrom<SourceFidelityWire> for SourceFidelity {
    type Error = String;

    fn try_from(wire: SourceFidelityWire) -> Result<Self, Self::Error> {
        let fidelity = Self {
            decode_digest: wire.decode_digest,
            file_digests: wire.file_digests,
            content_digest: wire.content_digest,
            codec_digests: wire.codec_digests,
            annotations: wire.annotations,
            retained_records: wire.retained_records,
        };
        let derived = fidelity.unrepresented();
        if wire.unrepresented != derived {
            return Err(format!(
                "unrepresented classes [{}] differ from the union of the retained records' \
                 untyped classes [{}]",
                class_list(&wire.unrepresented),
                class_list(&derived),
            ));
        }
        Ok(fidelity)
    }
}

/// Returns the sidecar wire name of an arena class (`model`, `design`, or
/// `process`).
#[must_use]
pub const fn arena_class_name(class: ArenaClass) -> &'static str {
    match class {
        ArenaClass::Model => "model",
        ArenaClass::Design => "design",
        ArenaClass::Process => "process",
    }
}

/// Returns the sidecar wire names of `classes`, comma-separated, for a message.
pub(crate) fn class_list(classes: &BTreeSet<ArenaClass>) -> String {
    classes
        .iter()
        .map(|class| arena_class_name(*class))
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Serialize)]
struct SourceFidelityWriteWire<'a> {
    decode_digest: &'a Sha256Digest,
    file_digests: &'a BTreeMap<ProductDefinitionId, Sha256Digest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content_digest: Option<&'a Sha256Digest>,
    codec_digests: &'a CodecDigests,
    #[serde(serialize_with = "cadmpeg_core::distinct_keys::set::serialize")]
    unrepresented: BTreeSet<ArenaClass>,
    annotations: &'a Annotations,
    retained_records: &'a BTreeMap<UnknownId, RetainedSourceRecord>,
}

impl SourceFidelity {
    fn serialize_erased(
        &self,
        serializer: cadmpeg_core::serde_erase::SerAdapter<'_>,
    ) -> Result<(), cadmpeg_core::serde_erase::SerError> {
        SourceFidelityWriteWire {
            decode_digest: &self.decode_digest,
            file_digests: &self.file_digests,
            content_digest: self.content_digest.as_ref(),
            codec_digests: &self.codec_digests,
            unrepresented: self.unrepresented(),
            annotations: &self.annotations,
            retained_records: &self.retained_records,
        }
        .serialize(serializer)
    }
}

impl Serialize for SourceFidelity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        cadmpeg_core::serde_erase::serialize_with(serializer, |adapter| {
            self.serialize_erased(adapter)
        })
    }
}

impl SourceFidelity {
    /// Fidelity of a decode whose root file has the given digest, with no
    /// annotations and no retained records.
    #[must_use]
    pub fn new(decode_digest: Sha256Digest) -> Self {
        Self::with_annotations(Annotations::default(), decode_digest)
    }

    /// Fidelity of a decode of the acquired root view: its digest is the
    /// SHA-256 of the root file's bytes.
    #[must_use]
    pub fn for_root(root: View<'_>) -> Self {
        Self::new(Sha256Digest::digest(root.window()))
    }

    /// Fidelity carrying the given annotations, for a decode whose root file
    /// has the given digest.
    ///
    /// The decode wrapper sets `content_digest`; until then it is absent,
    /// and a document compared with it reads as edited.
    #[must_use]
    pub fn with_annotations(annotations: Annotations, decode_digest: Sha256Digest) -> Self {
        Self {
            decode_digest,
            file_digests: BTreeMap::new(),
            content_digest: None,
            codec_digests: CodecDigests::new(),
            annotations,
            retained_records: BTreeMap::new(),
        }
    }

    /// Consume the source metadata without copying its retained bytes.
    pub fn into_parts(self) -> (Annotations, BTreeMap<UnknownId, RetainedSourceRecord>) {
        (self.annotations, self.retained_records)
    }

    /// Returns the classes whose source content survives in no typed document
    /// record: the union of the retained records' untyped classes.
    #[must_use]
    pub fn unrepresented(&self) -> BTreeSet<ArenaClass> {
        self.retained_records
            .values()
            .flat_map(|record| record.untyped.iter().copied())
            .collect()
    }

    /// Marks a retained record as kept without decoding, with the content
    /// classes its bytes can hold. The classes join any already marked.
    pub fn mark_untyped(
        &mut self,
        id: &str,
        classes: impl IntoIterator<Item = ArenaClass>,
    ) -> Result<(), NativeConvertError> {
        let record = self.retained_records.get_mut(id).ok_or_else(|| {
            NativeConvertError::InvalidCollection(format!("no retained source record {id}"))
        })?;
        record.untyped.extend(classes);
        Ok(())
    }

    /// Whether `ir` is edited: whether `content_digest` is absent or differs
    /// from the SHA-256 of its canonical JSON (§3.1, Rule B). A document that
    /// has no canonical JSON, because it holds a non-finite number or nests
    /// past the depth a default session admits, is edited. So is a document whose
    /// arenas the CADIR wire omits differ from the baseline the decode wrapper
    /// recorded in memory ([`crate::hash::omitted_arenas_edited`]).
    ///
    /// # Errors
    ///
    /// A session ceiling of `ctx` ran out.
    pub fn edited(&self, ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<bool, CodecError> {
        let same = self.content_digest.as_ref().is_some_and(|stored| {
            crate::hash::content_digest(ctx, ir).is_ok_and(|digest| digest == *stored)
        });
        Ok(!same || crate::hash::omitted_arenas_edited(ctx, ir).unwrap_or(true))
    }

    /// Append source metadata after checking every table for collisions.
    /// Both sides must describe the same root file, and a component file
    /// stated on both sides, or a codec digest stated on both sides, must
    /// have one digest. Failure leaves this source metadata unchanged.
    pub fn append(&mut self, mut other: Self) -> Result<(), cadmpeg_core::CodecError> {
        if self.decode_digest != other.decode_digest {
            return Err(cadmpeg_core::CodecError::malformed(format_args!(
                "source fidelity of root file {} cannot take the records of root file {}",
                self.decode_digest, other.decode_digest
            )));
        }
        for (definition, digest) in &other.file_digests {
            if self
                .file_digests
                .get(definition)
                .is_some_and(|existing| existing != digest)
            {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "component file of {definition} has two digests"
                )));
            }
        }
        for (key, digest) in &other.codec_digests {
            if self
                .codec_digests
                .get(key)
                .is_some_and(|existing| existing != digest)
            {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "codec digest {key} has two values"
                )));
            }
        }
        for id in other.retained_records.keys() {
            if self.retained_records.contains_key(id) {
                return Err(cadmpeg_core::CodecError::malformed(format_args!(
                    "duplicate retained or native unknown record {id}"
                )));
            }
        }
        self.annotations.append(other.annotations)?;
        self.file_digests.append(&mut other.file_digests);
        self.codec_digests.append(&mut other.codec_digests);
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

    /// Stores source bytes and native references.
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
        let codec = crate::native::namespace_codec(format)?;
        let mut products: Vec<crate::NativeUnknownRecord> = match ir.native.namespace(format) {
            Some(namespace) => namespace.arena_as_for_decode(ctx, "unknowns")?,
            None => Vec::new(),
        };
        let existing_ids: BTreeSet<&str> = ir
            .native
            .0
            .values()
            .filter_map(|namespace| namespace.arenas().get("unknowns"))
            .flatten()
            .map(crate::native::NativeRecord::id)
            .collect();
        let mut retained = BTreeMap::new();
        for record in records {
            if self.retained_records.contains_key(record.id())
                || existing_ids.contains(record.id().as_str())
                || retained.contains_key(record.id())
            {
                return Err(duplicate_record(record.id()).into());
            }
            let id = record.id().clone();
            let mut links = Vec::new();

            for link in record.links() {
                let text = link.to_owned();
                links.push(crate::ids::Identity::new(text).map_err(CodecError::malformed)?);
            }

            products.push(crate::NativeUnknownRecord { id, links });
            let stream =
                if let Some(provenance) = self.annotations.provenance.get(record.id().as_str()) {
                    SourceOwner::from(provenance.stream().to_owned())
                } else {
                    SourceOwner::Root
                };
            let (id, record) = RetainedSourceRecord::from_unknown(stream, record)?;
            retained.insert(id, record);
        }

        let mut native_records = Vec::new();

        for product in &products {
            let id = product.id.clone().into();
            let mut fields = serde_json::Map::new();
            let links = product
                .links
                .iter()
                .map(|link| serde_json::Value::String(link.as_str().to_owned()))
                .collect();
            fields.insert("links".to_owned(), serde_json::Value::Array(links));
            native_records.push(crate::native::NativeRecord::new(id, fields)?);
        }
        native_records.sort_by(|left, right| left.id().cmp(right.id()));
        if let Some(pair) = native_records
            .windows(2)
            .find(|pair| pair[0].id() == pair[1].id())
        {
            let id = pair[0].id();

            return Err(NativeConvertError::InvalidCollection(format!(
                "duplicate native unknown record {id}"
            ))
            .into());
        }

        // The native product text of each appended record bypasses the record
        // serializer, so count it here, before either destination changes. The
        // codec that copied the source bytes counted them at the copy.
        for record in &native_records {
            if retained.contains_key(record.id()) {
                ctx.retain_bytes(u64_from_index(record.text_len()), ATTACH_UNKNOWN_RECORD)?;
            }
        }

        let arena_name = cadmpeg_core::nonblank_literal!("unknowns");

        ir.native
            .namespace_mut(codec)
            .arenas_mut()
            .insert(arena_name, native_records);
        self.retained_records.extend(retained);
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
    mod digests;
    mod unknown_keys;
    mod unrepresented;

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

    fn root_digest() -> Sha256Digest {
        Sha256Digest::digest(b"test source")
    }

    fn fidelity() -> SourceFidelity {
        SourceFidelity::new(root_digest())
    }

    /// The text of a fidelity of [`root_digest`] holding `records`, the text of
    /// its `retained_records` object, none of them untyped.
    fn fidelity_text(records: &str) -> String {
        format!(
            r#"{{"decode_digest":"{}","file_digests":{{}},
                "codec_digests":{{}},"unrepresented":[],
                "annotations":{{"provenance":{{}},"exactness":{{}}}},"retained_records":{records}}}"#,
            root_digest()
        )
    }

    fn report() -> DecodeReport {
        DecodeReport::unclassified(
            crate::codec_id!("test"),
            crate::report::decode::DecodeTransfer::full(true),
            std::collections::BTreeMap::default(),
            Vec::new(),
            Vec::new(),
            crate::report::decode::TransferLedger::default(),
        )
    }

    #[test]
    fn retained_records_are_keyed_by_their_id() {
        let mut sidecar = fidelity();
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
        let error = serde_json::from_str::<SourceFidelity>(&fidelity_text(
            r#"{
                 "synthetic:source:record#a":{"stream":"source","offset":0,
                      "bytes":{"kind": "inline","data":"AQ=="},"untyped":[]},
                 "synthetic:source:record#a":{"stream":"source","offset":0,
                      "bytes":{"kind": "inline","data":"Ag=="},"untyped":[]}}"#,
        ))
        .expect_err("a repeated record id states two records under one identity")
        .to_string();
        assert!(
            error.contains("duplicate key synthetic:source:record#a"),
            "{error}"
        );

        let single = serde_json::from_str::<SourceFidelity>(&fidelity_text(
            r#"{
                 "synthetic:source:record#a":{"stream":"source","offset":0,
                      "bytes":{"kind": "inline","data":"Ag=="},"untyped":[]}}"#,
        ))
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
        assert_eq!(wire["bytes"]["kind"], "inline");
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
                "retention": {"kind": "inline", "data": "AQID"},
                "links": []
            }))
            .unwrap();
            let mut fidelity = fidelity();
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
                    "kind": "digest", "byte_len": 3,
                    "sha256": crate::hash::sha256_hex(&[1, 2, 3])
                },
                "untyped": []
            })
        };
        let one: SourceFidelity = serde_json::from_str(&fidelity_text(&format!(
            r#"{{"synthetic:source:record#a":{}}}"#,
            record("s")
        )))
        .expect("one record reads");
        assert_eq!(one.retained_records.len(), 1);

        let text = fidelity_text(&format!(
            r#"{{"synthetic:source:record#a":{},"synthetic:source:record#a":{}}}"#,
            record("s"),
            record("t")
        ));
        let error = serde_json::from_str::<SourceFidelity>(&text)
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
            fidelity(),
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
            fidelity(),
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
        let stream = crate::annotations::StreamHandle::new(crate::stream_name!(" \t"));
        builder.note("synthetic:point#0", &stream, 17, Some("point"));
        let sidecar = DecodeSidecar::bind_sha256(
            crate::hash::digest::Sha256Digest::digest(ir_json.as_bytes()),
            report(),
            SourceFidelity::with_annotations(builder.build(), root_digest()),
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
            fidelity(),
        );
        let json = sidecar.to_canonical_json().expect("serialize sidecar");
        assert!(
            json.contains("\"kind\": \"unclassified\"") && json.contains("\"format\": \"test\""),
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
            fidelity(),
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
        let mut fidelity = fidelity();
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
