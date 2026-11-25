// SPDX-License-Identifier: Apache-2.0
//! Content hashing helpers shared by codecs.

use std::collections::{BTreeMap, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::document::{
    CadIr, SortedDesign, SortedModel, SortedProcess, SortedTopology, SourceMeta,
};
use crate::native::{Native, NativeConvertError, NativeRecord};
use crate::units::{CanonicalUnitsWire, Tolerances};

pub mod digest;
pub mod finite_json;

use finite_json::CanonicalJsonError;

/// Borrowed unique owners. Repeated construction references have no owner.
pub(crate) struct ProceduralOwners<'a, T>(HashMap<&'a str, Option<&'a T>>);

impl<'a, T> ProceduralOwners<'a, T> {
    pub(crate) fn get(&self, construction: &str) -> Option<&'a T> {
        self.0.get(construction).copied().flatten()
    }
}

/// Index owners by construction; a repeated construction keeps no owner.
pub(crate) fn procedural_owners<'a, T>(
    owners: impl IntoIterator<Item = (&'a str, &'a T)>,
) -> ProceduralOwners<'a, T> {
    let mut table = HashMap::new();
    for (key, owner) in owners {
        table
            .entry(key)
            .and_modify(|slot| *slot = None)
            .or_insert(Some(owner));
    }
    ProceduralOwners(table)
}

/// Returns the SHA-256 digest of `bytes`.
#[must_use]
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Returns the lowercase hexadecimal SHA-256 digest of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    LowerHex(&sha256(bytes)).to_string()
}

/// Hash canonical pretty JSON under the caller's depth limit.
pub fn canonical_json_sha256<T: Serialize + ?Sized>(
    ctx: &DecodeContext<'_>,
    value: &T,
) -> Result<String, DigestError> {
    canonical_json_digest(ctx, value).map(Into::into)
}

/// Returns the SHA-256 of the canonical JSON of `ir` (§2.5): the digest
/// `SourceFidelity::content_digest` records (§3.1, Rule B). `ir` is hashed in
/// the arena order it holds, so a document whose arenas are not sorted by id
/// hashes as one whose canonical JSON differs.
///
/// # Errors
///
/// `ir` has no canonical JSON: it holds a non-finite number, or it nests past
/// the depth `ctx` admits.
pub fn content_digest(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<digest::Sha256Digest, DigestError> {
    let bytes = cadmpeg_core::finite_json::canonical_json_sha256_erased(
        Some((ctx, "walk document digest")),
        &|serializer| ir.serialize_erased(serializer),
    )
    .map_err(digest_error_from_finite)?;
    Ok(digest::Sha256Digest::from_bytes(bytes))
}

/// Returns [`content_digest`], and the records of `ir` past a bound of §2.5,
/// found in the same walk. A refused walk returns what it found before the
/// refusal.
pub(crate) fn measured_content_digest(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> (
    Result<digest::Sha256Digest, DigestError>,
    crate::bounds::Measured,
) {
    let mut buffered = std::io::BufWriter::with_capacity(
        cadmpeg_core::finite_json::DIGEST_BUFFER_BYTES,
        DigestWriter(Sha256::new()),
    );
    let mut measure = crate::bounds::Measure::new(&mut buffered);
    let written = finite_json::write_canonical_json_erased(
        finite_json::admission::CanonicalAdmission::limited(ctx, "walk document digest"),
        &mut measure,
        &|serializer| ir.serialize_erased(serializer),
    );
    let measured = measure.finish();
    let digest = written
        .map_err(DigestError::from)
        .and_then(|()| finish_digest(buffered));
    (digest, measured)
}

/// The canonical JSON of `ir`, with each record it holds past a bound of
/// §2.5. The walk admits every depth.
pub(crate) fn measured_canonical_json(
    ir: &CadIr,
) -> Result<(Vec<u8>, crate::bounds::Measured), CanonicalJsonError> {
    let mut bytes = Vec::new();
    let mut measure = crate::bounds::Measure::new(&mut bytes);
    let written = finite_json::write_canonical_json_erased(
        finite_json::admission::CanonicalAdmission::unlimited(),
        &mut measure,
        &|serializer| ir.serialize_erased(serializer),
    );
    let measured = measure.finish();
    written?;
    Ok((bytes, measured))
}

/// The refusal of the canonical JSON of `value`, where its walk is refused.
/// The walk notes no bound for a later measured walk.
pub(crate) fn canonical_refusal<T: Serialize + ?Sized>(value: &T) -> Option<CanonicalJsonError> {
    let refusal = finite_json::write_canonical_json(
        finite_json::admission::CanonicalAdmission::unlimited(),
        &mut std::io::sink(),
        value,
    )
    .err();
    crate::bounds::take_expression_past();
    refusal
}

fn canonical_json_digest<T: Serialize + ?Sized>(
    ctx: &DecodeContext<'_>,
    value: &T,
) -> Result<digest::Sha256Digest, DigestError> {
    let bytes = cadmpeg_core::finite_json::canonical_json_sha256_erased(
        Some((ctx, "walk document digest")),
        &|serializer| value.serialize(serializer),
    )
    .map_err(digest_error_from_finite)?;
    Ok(digest::Sha256Digest::from_bytes(bytes))
}

struct DigestWriter(Sha256);

impl std::io::Write for DigestWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn finish_digest(
    buffered: std::io::BufWriter<DigestWriter>,
) -> Result<digest::Sha256Digest, DigestError> {
    let writer = buffered
        .into_inner()
        .map_err(|error| DigestError::Write(error.into_error()))?;
    Ok(digest::Sha256Digest::from_bytes(writer.0.finalize().into()))
}

fn digest_error_from_finite(error: cadmpeg_core::finite_json::FiniteJsonError) -> DigestError {
    match error {
        cadmpeg_core::finite_json::FiniteJsonError::Write(error) => DigestError::Write(error),
        other => DigestError::CanonicalJson(CanonicalJsonError::from(other)),
    }
}

/// A digest could not be computed.
///
/// A digested value can carry a raw `f64`. `serde_json` writes a non-finite one
/// as `null`, so the digest serializes through the same adapter the document
/// write route uses, which refuses it instead: see [`CanonicalJsonError`].
#[derive(Debug, thiserror::Error)]
pub enum DigestError {
    /// A record the unknown arena cannot state.
    #[error(transparent)]
    Record(#[from] NativeConvertError),
    /// The document has no canonical JSON.
    #[error(transparent)]
    CanonicalJson(#[from] CanonicalJsonError),
    /// The digest sink refused a byte.
    #[error("digest sink: {0}")]
    Write(std::io::Error),
}

impl From<DigestError> for cadmpeg_core::CodecError {
    fn from(error: DigestError) -> Self {
        match error {
            DigestError::Record(error) => error.into(),
            DigestError::CanonicalJson(CanonicalJsonError::Resource(error)) => error,
            error => Self::Malformed(error.to_string()),
        }
    }
}

/// Returns the machine-local content digest of `ir` as seen by the `format`
/// codec: the digest a codec compares between two documents it holds, or
/// records among its codec digests (§3.1, Rule B) before the decode wrapper
/// sets `scope`.
///
/// Covers the document in canonical arena order, without `scope` and `repro`,
/// with the `format` unknown arena reduced to identities and links and
/// `source_image_id` excluded. Retained source bytes never reach the digest.
/// The arenas the CADIR wire omits count only where they differ from the
/// baseline the decode wrapper recorded ([`omitted_arenas_local_sha256`]).
/// `source` states the metadata to include, including metadata not assigned
/// to `ir` yet. Normalization, ordering, serialization and hashing use `ctx`.
///
/// Bitwise SHA-256: not portable across platforms (libm last-place drift) and
/// not tolerance-aware (tolerant equality is not transitive).
pub fn document_local_sha256(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    source: Option<&SourceMeta>,
    format: &str,
    source_image_id: &str,
) -> Result<String, CodecError> {
    document_local_sha256_without_carried(ctx, ir, source, format, source_image_id, &[])
}

/// [`document_local_sha256`] with the `format` native arenas named in
/// `carried` left out.
///
/// An encoder that handles those native arenas itself, writing or refusing
/// each of their edits, records and compares this digest, so every other edit
/// changes the digest.
pub fn document_local_sha256_without_carried(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    source: Option<&SourceMeta>,
    format: &str,
    source_image_id: &str,
    carried: &[&str],
) -> Result<String, CodecError> {
    let unknowns = reduced_unknowns(ctx, ir, format, source_image_id)?;
    let omitted_arenas = edited_omitted_arenas(ctx, ir)?;
    let document = NormalizedDocument {
        ir_version: ir.ir_version(),
        fragment: ir.fragment,
        source: source.cloned(),
        units: CanonicalUnitsWire::default(),
        tolerances: &ir.tolerances,
        geometry: ir.geometry.sorted(),
        model: ir.model.sorted(),
        design: ir.design.sorted(),
        process: ir.process.sorted(),
        native: normalized_native(ctx, &ir.native, format, &unknowns, carried),
        omitted_arenas,
    };
    match canonical_json_sha256(ctx, &document) {
        Ok(digest) => Ok(digest),
        Err(error) => {
            if let Some(limit) = ctx.resource_refusal() {
                Err(limit.into())
            } else {
                Err(error.into())
            }
        }
    }
}

/// Returns the machine-local digest of the arenas the CADIR wire omits
/// ([`crate::document::ArenaName::WIRE_OMITTED`]) and the feature-parent
/// relations of their features, as `ir` holds them in memory.
///
/// [`content_digest`] covers the wire form of a document. The decode wrapper
/// records this digest beside it in memory, so an edit of these arenas still
/// makes the document edited ([`omitted_arenas_edited`]) and changes
/// [`document_local_sha256`]. Same bitwise, machine-local properties as
/// [`document_local_sha256`].
pub fn omitted_arenas_local_sha256(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<String, CodecError> {
    canonical_json_sha256(ctx, &ir.model.sorted_omitted()).map_err(|error| {
        ctx.resource_refusal()
            .map_or_else(|| error.into(), Into::into)
    })
}

/// The digest of the arenas the wire omits, where they differ from the
/// baseline the decode wrapper recorded. A document read from CADIR JSON
/// holds no baseline, so its digest is that of its wire form.
fn edited_omitted_arenas(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Option<String>, CodecError> {
    let Some(baseline) = ir.model.omitted_arenas_baseline() else {
        return Ok(None);
    };
    let digest = omitted_arenas_local_sha256(ctx, ir)?;
    Ok((baseline != digest).then_some(digest))
}

/// Whether the arenas the CADIR wire omits differ from the baseline the
/// decode wrapper recorded. A document with no baseline, such as one read
/// from CADIR JSON, has no such edit.
///
/// # Errors
///
/// The arenas have no canonical JSON under `ctx`.
pub fn omitted_arenas_edited(ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<bool, CodecError> {
    edited_omitted_arenas(ctx, ir).map(|digest| digest.is_some())
}

fn reduced_unknowns(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    format: &str,
    source_image_id: &str,
) -> Result<Vec<NativeRecord>, CodecError> {
    let mut reduced = Vec::new();
    if let Some(namespace) = ir.native.namespace(format) {
        if let Some(records) = namespace.arenas().get("unknowns") {
            for record in records {
                let normalized = record.digest_unknown(ctx)?;

                if record.id() == source_image_id {
                    continue;
                }
                reduced.push(normalized);
            }
        }
    }
    reduced.sort_by(|left, right| left.id().cmp(right.id()));
    Ok(reduced)
}

/// A document as the semantic digest sees it.
///
/// Mirrors [`CadIr`]'s serialized shape field for field, borrowing what it can,
/// except `scope`: decoders take this digest before the decode wrapper
/// records the scope.
#[derive(Serialize)]
struct NormalizedDocument<'a, 'input> {
    ir_version: &'a str,
    fragment: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<SourceMeta>,
    units: CanonicalUnitsWire,
    tolerances: &'a Tolerances,
    geometry: SortedTopology<'a>,
    model: SortedModel<'a>,
    design: SortedDesign<'a>,
    process: SortedProcess<'a>,
    native: BTreeMap<&'a str, BTreeMap<&'a str, DigestNativeArena<'a, 'input>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    omitted_arenas: Option<String>,
}

/// Serializes one native arena in `NormalizedDocument.native.<format>.<arena>`.
/// Large arenas format L2-cache-sized batches in parallel and emit each chunk
/// as a `RawValue` element inside `SerializeSeq`, avoiding giant buffers while
/// matching `serde_json::Serializer::pretty` byte-for-byte.
struct DigestNativeArena<'a, 'input> {
    ctx: &'a DecodeContext<'input>,
    records: Vec<&'a NativeRecord>,
}

struct RawJsonChunk<'a>(&'a str);

impl Serialize for RawJsonChunk<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("$serde_json::private::RawValue", 1)?;
        state.serialize_field("$serde_json::private::RawValue", self.0)?;
        state.end()
    }
}

impl Serialize for DigestNativeArena<'_, '_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;

        const PARALLEL_THRESHOLD: usize = 512;
        const CHUNK_RECORDS: usize = 512;
        let workers = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
        if self.records.len() < PARALLEL_THRESHOLD || workers <= 1 {
            let mut seq = serializer.serialize_seq(Some(self.records.len()))?;
            for &record in &self.records {
                seq.serialize_element(&DigestNativeRecord {
                    ctx: self.ctx,
                    record,
                })?;
            }
            return seq.end();
        }

        // Pipeline formatting on one dedicated producer thread over a bounded
        // channel so chunks stay hot in L2/L3 cache and formatting overlaps
        // continuously with main-thread SHA-256 compression without per-batch
        // thread-spawn overhead.
        let mut seq = serializer.serialize_seq(None)?;
        let mut max_depth = 1_usize;
        let records = &self.records;
        let (tx, rx) = std::sync::mpsc::sync_channel::<(String, usize)>(4);
        let serialize_result: Result<(), S::Error> = std::thread::scope(|scope| {
            scope.spawn(move || {
                for chunk in records.chunks(CHUNK_RECORDS) {
                    let est: usize = chunk
                        .iter()
                        .map(|r| r.json_text().len().min(usize::MAX / 4) * 2 + 12)
                        .sum();
                    let mut out = String::with_capacity(est);
                    let mut local_max = 1_usize;
                    for (i, &record) in chunk.iter().enumerate() {
                        if i > 0 {
                            out.push_str(",\n        ");
                        }
                        let d = pretty_format_compact_json(
                            record.json_text(),
                            NATIVE_RECORD_INDENT_LEVEL,
                            &mut out,
                        );
                        local_max = local_max.max(d);
                    }
                    if tx.send((out, local_max)).is_err() {
                        break;
                    }
                }
            });
            let mut res = Ok(());
            while let Ok((part, depth)) = rx.recv() {
                max_depth = max_depth.max(depth);
                if let Err(err) = seq.serialize_element(&RawJsonChunk(&part)) {
                    res = Err(err);
                    break;
                }
            }
            drop(rx);
            res
        });
        serialize_result?;
        // `SerializeSeq` entered 1 level and `RawJsonChunk` enters 2 levels;
        // enforce the exact same peak depth ceiling as per-record serialization.
        if max_depth > 2 {
            let extra = max_depth - 2;
            let enter = || {
                self.ctx.enter_nested("walk document digest").map_err(|_| {
                    serde::ser::Error::custom("canonical JSON resource admission refused")
                })
            };
            match extra {
                1 => {
                    let _g1 = enter()?;
                }
                2 => {
                    let _g1 = enter()?;
                    let _g2 = enter()?;
                }
                3 => {
                    let _g1 = enter()?;
                    let _g2 = enter()?;
                    let _g3 = enter()?;
                }
                4 => {
                    let _g1 = enter()?;
                    let _g2 = enter()?;
                    let _g3 = enter()?;
                    let _g4 = enter()?;
                }
                _ => {
                    let mut guards = Vec::with_capacity(extra);
                    for _ in 0..extra {
                        guards.push(enter()?);
                    }
                }
            }
        }
        seq.end()
    }
}

/// Serializes a stored canonical compact [`NativeRecord`] as pretty JSON at
/// indent level 4 (inside `NormalizedDocument.native.<format>.<arena>[i]`),
/// avoiding `serde_json::Deserializer` round-tripping while keeping the exact
/// bytes `serde_json::Serializer::pretty` + `FiniteSerializer` produce.
struct DigestNativeRecord<'a, 'input> {
    ctx: &'a DecodeContext<'input>,
    record: &'a NativeRecord,
}

/// Depth of `NormalizedDocument.native.<format>.<arena>[i]` in pretty JSON.
const NATIVE_RECORD_INDENT_LEVEL: usize = 4;

#[inline]
fn push_indent(out: &mut String, level: usize) {
    const SPACES: &str = "                                                                ";
    let mut remaining = level * 2;
    if remaining <= SPACES.len() {
        out.push_str(&SPACES[..remaining]);
        return;
    }
    while remaining >= SPACES.len() {
        out.push_str(SPACES);
        remaining -= SPACES.len();
    }
    out.push_str(&SPACES[..remaining]);
}

/// Format canonical compact JSON text into `serde_json::ser::PrettyFormatter`
/// text starting at `base_indent`, normalizing `-0.0` outside strings to
/// `0.0`. Returns the maximum serializer depth reached inside the value
/// (containers plus scalar leaves).
fn pretty_format_compact_json(compact: &str, base_indent: usize, out: &mut String) -> usize {
    let bytes = compact.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut depth = 0_usize;
    let mut max_depth = 1_usize;
    while i < len {
        match bytes[i] {
            b'"' => {
                let start = i;
                i += 1;
                let mut closed = false;
                while let Some(offset) = memchr::memchr2(b'"', b'\\', &bytes[i..]) {
                    let hit = i + offset;
                    if bytes[hit] == b'"' {
                        i = hit + 1;
                        closed = true;
                        break;
                    }
                    i = (hit + 2).min(len);
                }
                if !closed {
                    i = len;
                }
                out.push_str(&compact[start..i]);
                max_depth = max_depth.max(depth + 1);
            }
            b'{' => {
                if bytes.get(i + 1) == Some(&b'}') {
                    out.push_str("{}");
                    max_depth = max_depth.max(depth + 1);
                    i += 2;
                } else {
                    depth += 1;
                    max_depth = max_depth.max(depth);
                    out.push_str("{\n");
                    push_indent(out, base_indent + depth);
                    i += 1;
                }
            }
            b'[' => {
                if bytes.get(i + 1) == Some(&b']') {
                    out.push_str("[]");
                    max_depth = max_depth.max(depth + 1);
                    i += 2;
                } else {
                    depth += 1;
                    max_depth = max_depth.max(depth);
                    out.push_str("[\n");
                    push_indent(out, base_indent + depth);
                    i += 1;
                }
            }
            b'}' => {
                depth = depth.max(1) - 1;
                out.push('\n');
                push_indent(out, base_indent + depth);
                out.push('}');
                i += 1;
            }
            b']' => {
                depth = depth.max(1) - 1;
                out.push('\n');
                push_indent(out, base_indent + depth);
                out.push(']');
                i += 1;
            }
            b',' => {
                out.push_str(",\n");
                push_indent(out, base_indent + depth);
                i += 1;
            }
            b':' => {
                out.push_str(": ");
                i += 1;
            }
            b'-' if bytes.get(i..i + 4) == Some(b"-0.0")
                && matches!(bytes.get(i + 4), None | Some(b',' | b'}' | b']')) =>
            {
                out.push_str("0.0");
                max_depth = max_depth.max(depth + 1);
                i += 4;
            }
            _ => {
                let start = i;
                i += 1;
                while i < len && !matches!(bytes[i], b'"' | b'{' | b'}' | b'[' | b']' | b',' | b':')
                {
                    i += 1;
                }
                out.push_str(&compact[start..i]);
                max_depth = max_depth.max(depth + 1);
            }
        }
    }
    max_depth
}

impl Serialize for DigestNativeRecord<'_, '_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        use std::cell::RefCell;

        thread_local! {
            static PRETTY_SCRATCH: RefCell<String> = const { RefCell::new(String::new()) };
        }

        // `RawValue` serialization itself enters 2 levels (`serialize_struct` +
        // `serialize_str`); enter any remaining levels on `ctx` to enforce the
        // exact same recursion-depth ceiling.
        PRETTY_SCRATCH.with(|scratch| {
            let mut buf = scratch.borrow_mut();
            buf.clear();
            let text = self.record.json_text();
            let reserve = text.len().min(usize::MAX / 2) * 2;
            if buf.capacity() < reserve {
                buf.reserve(reserve);
            }
            let max_depth = pretty_format_compact_json(text, NATIVE_RECORD_INDENT_LEVEL, &mut buf);
            if max_depth > 2 {
                let extra = max_depth - 2;
                let enter = || {
                    self.ctx.enter_nested("walk document digest").map_err(|_| {
                        serde::ser::Error::custom("canonical JSON resource admission refused")
                    })
                };
                match extra {
                    1 => {
                        let _g1 = enter()?;
                    }
                    2 => {
                        let _g1 = enter()?;
                        let _g2 = enter()?;
                    }
                    3 => {
                        let _g1 = enter()?;
                        let _g2 = enter()?;
                        let _g3 = enter()?;
                    }
                    4 => {
                        let _g1 = enter()?;
                        let _g2 = enter()?;
                        let _g3 = enter()?;
                        let _g4 = enter()?;
                    }
                    _ => {
                        let mut guards = Vec::with_capacity(extra);
                        for _ in 0..extra {
                            guards.push(enter()?);
                        }
                    }
                }
            }
            let mut state = serializer.serialize_struct("$serde_json::private::RawValue", 1)?;
            state.serialize_field("$serde_json::private::RawValue", buf.as_str())?;
            state.end()
        })
    }
}

/// Borrow every native namespace in canonical order, replacing the `format`
/// unknown arena with `unknowns`, leaving out the `format` arenas named in
/// `carried`, and creating that namespace when the document has none.
fn normalized_native<'a, 'input>(
    ctx: &'a DecodeContext<'input>,
    native: &'a Native,
    format: &'a str,
    unknowns: &'a [NativeRecord],
    carried: &[&str],
) -> BTreeMap<&'a str, BTreeMap<&'a str, DigestNativeArena<'a, 'input>>> {
    let wrap = |records: &'a [NativeRecord]| -> DigestNativeArena<'a, 'input> {
        DigestNativeArena {
            ctx,
            records: sorted_records(records),
        }
    };
    let mut namespaces = BTreeMap::new();
    for (name, namespace) in &native.0 {
        let mut arenas = BTreeMap::new();
        for (arena, records) in namespace.arenas() {
            if name == format && (arena == "unknowns" || carried.contains(&arena.as_str())) {
                continue;
            }
            arenas.insert(arena.as_str(), wrap(records));
        }
        if name == format {
            arenas.insert(
                "unknowns",
                DigestNativeArena {
                    ctx,
                    records: unknowns.iter().collect(),
                },
            );
        }
        namespaces.insert(name.as_str(), arenas);
    }
    if !namespaces.contains_key(format) {
        let mut arenas = BTreeMap::new();
        arenas.insert(
            "unknowns",
            DigestNativeArena {
                ctx,
                records: unknowns.iter().collect(),
            },
        );
        namespaces.insert(format, arenas);
    }
    namespaces
}

fn sorted_records(records: &[NativeRecord]) -> Vec<&NativeRecord> {
    let mut refs = records.iter().collect::<Vec<_>>();
    refs.sort_by(|left, right| left.id().cmp(right.id()));
    refs
}

/// Borrowed bytes displayed and serialized as two lowercase hexadecimal digits per byte.
pub struct LowerHex<'a>(pub &'a [u8]);

impl std::fmt::Display for LowerHex<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl Serialize for LowerHex<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::finite_json::CanonicalJsonError;
    use super::{canonical_json_sha256, document_local_sha256, sha256_hex, DigestError};
    use crate::document::CadIr;
    use crate::examples::unit_cube;
    use crate::ids::UnknownId;
    use crate::native::{Native, NativeRecord};
    use crate::unknown::UnknownRecord;
    use cadmpeg_core::CodecError;

    /// The document digest counts the arenas the wire omits where they differ
    /// from the baseline the decode wrapper recorded, so an edit of them in
    /// memory is seen, and a document read back from CADIR JSON digests as its
    /// stamp.
    #[test]
    fn omitted_arenas_count_in_memory_and_not_after_a_cadir_read() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let digest = |ir: &CadIr| {
            document_local_sha256(&ctx, ir, None, "test", "test:file:source#0").unwrap()
        };
        let mut decoded = crate::examples::unit_cube_with_history_body().unwrap();
        decoded.finalize();
        assert!(!decoded.model.history_bodies.is_empty());
        let baseline = super::omitted_arenas_local_sha256(&ctx, &decoded).unwrap();
        decoded.model.record_omitted_arenas_baseline(baseline);
        let stamp = digest(&decoded);

        let read = CadIr::from_json(&decoded.to_canonical_json().unwrap()).unwrap();
        assert!(decoded.model.omitted_arenas_held());
        assert!(!read.model.omitted_arenas_held());
        assert!(read.model.history_bodies.is_empty());
        assert_eq!(digest(&read), stamp);

        let mut edited = decoded.clone();
        edited.model.history_bodies.clear();
        assert_ne!(digest(&edited), stamp);

        let mut unrecorded = edited.clone();
        unrecorded.model.omitted = crate::document::OmittedArenas::default();
        assert_eq!(digest(&unrecorded), stamp);
        ctx.finish_session().unwrap();
    }

    #[test]
    fn canonical_json_sha256_matches_json_larger_than_the_buffer() {
        let value: Vec<String> = (0..6_000).map(|index| format!("entry-{index}")).collect();
        let json = crate::hash::finite_json::to_canonical_json_string(&value).unwrap();
        assert!(json.len() > cadmpeg_core::finite_json::DIGEST_BUFFER_BYTES);
        assert_eq!(
            canonical_json_sha256(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &value
            )
            .unwrap(),
            crate::hash::sha256_hex(json.as_bytes())
        );
    }

    #[test]
    fn a_finite_float_digests() {
        assert!(canonical_json_sha256(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &1.0f64
        )
        .is_ok());
        assert!(canonical_json_sha256(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &vec![1.0f64, -2.5]
        )
        .is_ok());
    }

    #[test]
    fn a_non_finite_float_has_no_digest() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let Err(DigestError::CanonicalJson(CanonicalJsonError::NonFinite { value: refused })) =
                canonical_json_sha256(
                    &cadmpeg_core::decode::test_support::service_decode_context(),
                    &value,
                )
            else {
                panic!("a non-finite float must have no canonical JSON");
            };
            assert_eq!(refused.is_nan(), value.is_nan());
            assert_eq!(refused.is_infinite(), value.is_infinite());
        }
    }

    #[test]
    fn lower_hex_display_and_serialization() {
        for (bytes, expected) in [
            (&[][..], ""),
            (&[0x00, 0x01, 0x0f, 0x10, 0xab, 0xff][..], "00010f10abff"),
        ] {
            let hex = super::LowerHex(bytes);
            assert_eq!(hex.to_string(), expected);
            assert_eq!(
                serde_json::to_value(&hex).unwrap(),
                serde_json::json!(expected)
            );
        }
    }

    #[test]
    fn a_nested_non_finite_float_has_no_digest() {
        let nested = serde_json::json!({ "outer": [{ "inner": 1.0 }] });
        assert!(canonical_json_sha256(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &nested
        )
        .is_ok());
        let nested = vec![Some(vec![(1.0f64, f64::NAN)])];
        assert!(matches!(
            canonical_json_sha256(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &nested
            ),
            Err(DigestError::CanonicalJson(
                CanonicalJsonError::NonFinite { .. }
            ))
        ));
    }

    #[test]
    fn encodes_sha256_as_lowercase_hexadecimal() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// Record covering JSON shapes whose rendering could drift.
    fn pinned_record() -> NativeRecord {
        let serde_json::Value::Object(fields) = serde_json::json!({
            "zeta": null,
            "alpha": [-1, 0, 1.5, 2.0, 1e10, 1e-7],
            "beta": {
                "nested": {"deep": [true, false]},
                "empty_array": [],
                "empty_object": {}
            },
            "escaped": "quote\" backslash\\ slash/ newline\n tab\t bell\u{7} accent é",
            "gamma": 9_007_199_254_740_993_u64,
            "delta": -9_007_199_254_740_993_i64
        }) else {
            panic!("the pinned record literal is a JSON object");
        };
        NativeRecord::new(
            crate::ids::Identity::new("pin:test:record#0").expect("valid identity"),
            fields,
        )
        .expect("valid native identity")
    }

    fn pinned_native() -> Native {
        let mut native = Native::default();
        let namespace = native.namespace_mut(crate::codec_id!("pin"));
        namespace.arenas_mut().insert(
            cadmpeg_core::nonblank_literal!("records"),
            vec![pinned_record()],
        );
        native
    }

    /// Pretty-printed `NativeRecord` bytes covered by the document digest.
    #[test]
    fn pins_pretty_printed_native_record_bytes() {
        let expected = r#"{
  "id": "pin:test:record#0",
  "alpha": [
    -1,
    0,
    1.5,
    2.0,
    10000000000.0,
    1e-7
  ],
  "beta": {
    "empty_array": [],
    "empty_object": {},
    "nested": {
      "deep": [
        true,
        false
      ]
    }
  },
  "delta": -9007199254740993,
  "escaped": "quote\" backslash\\ slash/ newline\n tab\t bell\u0007 accent é",
  "gamma": 9007199254740993,
  "zeta": null
}"#;
        assert_eq!(
            serde_json::to_string_pretty(&pinned_record()).unwrap(),
            expected
        );
    }

    /// Same record indented inside namespace and arena, as hashing sees it.
    #[test]
    fn pins_pretty_printed_native_arena_bytes() {
        let expected = r#"{
  "pin": {
    "records": [
      {
        "id": "pin:test:record#0",
        "alpha": [
          -1,
          0,
          1.5,
          2.0,
          10000000000.0,
          1e-7
        ],
        "beta": {
          "empty_array": [],
          "empty_object": {},
          "nested": {
            "deep": [
              true,
              false
            ]
          }
        },
        "delta": -9007199254740993,
        "escaped": "quote\" backslash\\ slash/ newline\n tab\t bell\u0007 accent é",
        "gamma": 9007199254740993,
        "zeta": null
      }
    ]
  }
}"#;
        assert_eq!(
            serde_json::to_string_pretty(&pinned_native()).unwrap(),
            expected
        );
    }

    /// Digest over the arena above; formatting drift under
    /// `canonical_json_sha256` fails here.
    #[test]
    fn pins_native_arena_digest() {
        assert_eq!(
            canonical_json_sha256(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &pinned_native()
            )
            .unwrap(),
            "7249c236a39ac27b8614a9ef11d6b1e1c416e1242d909e6d0e94a96c1d6507d4"
        );
    }

    /// An admitted product projection of a retained source record.
    fn pinned_unknown(id: &str, links: &[&str]) -> NativeRecord {
        NativeRecord::from(&crate::NativeUnknownRecord {
            id: UnknownId::mint(id).expect("valid fixture identity"),
            links: links
                .iter()
                .map(|link| crate::ids::Identity::new(*link).expect("valid fixture link"))
                .collect(),
        })
    }

    fn pinned_document() -> CadIr {
        let mut ir = CadIr::empty();
        ir.native = pinned_native();
        let namespace = ir.native.namespace_mut(crate::codec_id!("pin"));
        namespace.arenas_mut().insert(
            cadmpeg_core::nonblank_literal!("unknowns"),
            vec![
                pinned_unknown("pin:test:source-image#0", &[]),
                pinned_unknown(
                    "pin:test:unknown#0",
                    &["pin:test:record#0", "pin:test:unknown#1"],
                ),
            ],
        );
        ir.finalize();
        ir
    }

    /// A document carrying one `pin` unknown record with the given fields.
    fn pinned_document_with_unknown(fields: serde_json::Map<String, serde_json::Value>) -> CadIr {
        let mut ir = pinned_document();
        ir.native
            .namespace_mut(crate::codec_id!("pin"))
            .arenas_mut()
            .insert(
                cadmpeg_core::nonblank_literal!("unknowns"),
                vec![NativeRecord::new(
                    crate::ids::Identity::new("pin:model:record#0").expect("valid identity"),
                    fields,
                )
                .expect("valid native identity")],
            );
        ir.finalize();
        ir
    }

    /// An arena whose records cannot be read has no digest. Reducing it to
    /// empty would make it collide with a document that carries no unknown
    /// records at all, and that digest is the write path's edit oracle.
    #[test]
    fn an_unreadable_unknown_arena_has_no_digest() {
        let source_image = "pin:test:source-image#0";
        let absent = document_local_sha256(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &pinned_document(),
            pinned_document().source.as_ref(),
            "pin",
            source_image,
        )
        .unwrap();

        let mut readable_fields = serde_json::Map::new();
        readable_fields.insert("links".into(), serde_json::json!([]));
        let readable_ir = pinned_document_with_unknown(readable_fields);
        let readable = document_local_sha256(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &readable_ir,
            readable_ir.source.as_ref(),
            "pin",
            source_image,
        )
        .unwrap();
        assert_ne!(readable, absent);

        let mut unreadable_fields = serde_json::Map::new();
        unreadable_fields.insert("links".into(), serde_json::json!(7));
        let unreadable = pinned_document_with_unknown(unreadable_fields);
        assert!(document_local_sha256(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &unreadable,
            unreadable.source.as_ref(),
            "pin",
            source_image
        )
        .is_err());
        assert!(document_local_sha256(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &unreadable,
            unreadable.source.as_ref(),
            "pin",
            source_image
        )
        .is_err());

        // Source retention fields belong to the sidecar. They cannot be
        // silently reduced out of a reserved product unknown record.
        let serde_json::Value::Object(source_fields) = serde_json::json!({
            "links": [], "offset": 4096, "byte_len": 12,
            "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
            "data": "AQID"
        }) else {
            unreachable!("object fixture")
        };
        let raw_source = pinned_document_with_unknown(source_fields);
        assert!(document_local_sha256(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &raw_source,
            raw_source.source.as_ref(),
            "pin",
            source_image
        )
        .is_err());
    }

    /// Pins both digest entry points over one fixed, platform-independent
    /// document.
    #[test]
    fn pins_document_digests() {
        let ir = pinned_document();
        assert_eq!(
            canonical_json_sha256(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &ir
            )
            .unwrap(),
            "010be63272d49ca6d7cd8efd051e25a9c70dc96891e497140fe0a8e83002ce2e"
        );
        assert_eq!(
            document_local_sha256(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &ir,
                ir.source.as_ref(),
                "pin",
                "pin:test:source-image#0"
            )
            .unwrap(),
            "06d03ae1af4c5675976ddf2ae73c2b789071c225a5e52405d4347c4026d3b219"
        );
    }

    #[test]
    fn document_digest_retains_only_the_completed_hexadecimal_text() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let ir = pinned_document_with_source();
        let expected = cloned_local_digest(&ir, "pin", "pin:test:source-image#0");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let actual = document_local_sha256(
            &ctx,
            &ir,
            ir.source.as_ref(),
            "pin",
            "pin:test:source-image#0",
        )
        .unwrap();
        assert_eq!(actual, expected);
        ctx.finish_session().unwrap();
    }

    /// The pinned document with the source metadata a decoded document carries:
    /// one ordinary attribute the digest keeps.
    fn pinned_document_with_source() -> CadIr {
        let mut ir = pinned_document();
        ir.source = Some(crate::document::SourceMeta::classified(
            cadmpeg_core::dialect::DialectLayers::of(
                cadmpeg_core::dialect::DialectMatch::admitted(cadmpeg_core::dialect_id!(
                    "pin:test"
                )),
            ),
            [(
                cadmpeg_core::nonblank_literal!("file_size"),
                "4096".to_owned(),
            )]
            .into_iter()
            .collect(),
        ));
        ir
    }

    /// Pins normalization over source metadata: every attribute is kept.
    ///
    /// The pin covers the canonical JSON of the normalized document, in which
    /// the source block reads
    ///
    /// ```text
    ///   "source": {
    ///     "identity": {
    ///       "kind": "classified",
    ///       "dialects": {
    ///         "primary": {
    ///           "dialect": "pin:test",
    ///           "declared": {},
    ///           "admission": {"kind": "admitted"}
    ///         },
    ///         "extra": []
    ///       }
    ///     },
    ///     "attributes": {
    ///       "file_size": "4096"
    ///     }
    ///   },
    /// ```
    ///
    /// The identity states the format once, in the dialect id its classified
    /// payload carries. The other
    /// pinned documents carry no source metadata, so their normalized form still
    /// elides the whole `source` member.
    #[test]
    fn pins_document_digest_over_source_metadata() {
        let ir = pinned_document_with_source();
        let independently_normalized = cloned_local_digest(&ir, "pin", "pin:test:source-image#0");
        assert_eq!(
            independently_normalized,
            "85c289406639a42ce038ec504207462a80f5fb5a645a4ef5c78d126e1282f0ff"
        );
        assert_eq!(
            document_local_sha256(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &ir,
                ir.source.as_ref(),
                "pin",
                "pin:test:source-image#0"
            )
            .unwrap(),
            independently_normalized
        );
    }

    #[test]
    fn local_source_digest_matches_an_assigned_source_without_cloning_the_document() {
        let mut ir = pinned_document_with_source();
        let source = ir.source.take().expect("fixture carries source metadata");

        assert_eq!(
            document_local_sha256(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &ir,
                Some(&source),
                "pin",
                "pin:test:source-image#0"
            )
            .unwrap(),
            document_local_sha256(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &pinned_document_with_source(),
                pinned_document_with_source().source.as_ref(),
                "pin",
                "pin:test:source-image#0"
            )
            .unwrap()
        );
    }

    /// Nothing about a record's rendering may depend on how it was built: a
    /// record parsed back out of a document must hash exactly as the one that
    /// produced it.
    #[test]
    fn round_tripped_document_hashes_identically() {
        let ir = pinned_document();
        let json = ir.to_canonical_json().unwrap();
        let mut reparsed = CadIr::from_json(&json).unwrap();
        reparsed.finalize();
        assert_eq!(
            canonical_json_sha256(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &ir
            )
            .unwrap(),
            canonical_json_sha256(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &reparsed
            )
            .unwrap()
        );
        assert_eq!(ir.to_canonical_json().unwrap(), json);
    }

    /// Copy the document, finalize order, drop the retained source image, and
    /// hash the serialized string.
    fn cloned_local_digest(ir: &CadIr, format: &str, source_image_id: &str) -> String {
        let mut normalized = ir.clone();
        normalized.finalize();
        let unknowns = ir
            .native_unknowns(format)
            .expect("the normalization fixture has admitted product unknowns")
            .into_iter()
            .filter(|record| record.id.as_str() != source_image_id)
            .collect::<Vec<_>>();
        normalized
            .set_native_unknowns(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                format,
                &unknowns,
            )
            .unwrap();
        let text = normalized.to_canonical_json().unwrap();
        let start = text
            .find("\n  \"scope\": {")
            .expect("the document records its scope");
        let end = start
            + text[start..]
                .find("\n  },")
                .expect("the scope object closes")
            + 5;
        crate::hash::sha256_hex(format!("{}{}", &text[..start], &text[end..]).as_bytes())
    }

    /// A document with an unordered model, a recorded digest, two native
    /// namespaces, and a retained source image among the unknown records.
    fn local_digest_fixture_with_source_image(
        source_image: UnknownRecord,
    ) -> (CadIr, crate::SourceFidelity) {
        let mut ir = unit_cube().expect("valid unit cube fixture");
        ir.geometry.faces.reverse();
        ir.geometry.surfaces.reverse();
        ir.source = Some(crate::SourceMeta::classified(
            cadmpeg_core::dialect::DialectLayers::of(
                cadmpeg_core::dialect::DialectMatch::admitted(cadmpeg_core::dialect_id!(
                    "synthetic:test"
                )),
            ),
            [(
                cadmpeg_core::nonblank_literal!("active_brep"),
                "body#0".to_owned(),
            )]
            .into_iter()
            .collect(),
        ));
        let body_id = ir.geometry.bodies[0].id.as_str().to_owned();
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test context");
        let mut source_fidelity =
            crate::SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(&[]));
        source_fidelity
            .attach_native_unknown_records(
                &mut ir,
                "synthetic",
                [
                    source_image,
                    UnknownRecord::retained(
                        UnknownId::mint("synthetic:model:record#1").expect("valid identity"),
                        8,
                        vec![4, 5],
                        vec![body_id],
                    ),
                ]
                .into(),
                &ctx,
            )
            .unwrap();
        let namespace = ir.native.namespace_mut(crate::codec_id!("other"));
        namespace.arenas_mut().insert(
            cadmpeg_core::nonblank_literal!("records"),
            vec![NativeRecord::new(
                crate::ids::Identity::new("other:test:record#0").expect("valid identity"),
                serde_json::Map::new(),
            )
            .expect("valid native identity")],
        );
        (ir, source_fidelity)
    }

    fn local_digest_fixture() -> (CadIr, crate::SourceFidelity) {
        local_digest_fixture_with_source_image(UnknownRecord::retained(
            UnknownId::mint("synthetic:file:source-image#0").expect("valid identity"),
            0,
            vec![1, 2, 3],
            Vec::new(),
        ))
    }

    #[test]
    fn document_local_sha256_matches_the_cloned_normalization() {
        let (ir, _source_fidelity) = local_digest_fixture();
        let source_image = "synthetic:file:source-image#0";
        assert_eq!(
            document_local_sha256(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &ir,
                ir.source.as_ref(),
                "synthetic",
                source_image
            )
            .unwrap(),
            cloned_local_digest(&ir, "synthetic", source_image)
        );
        assert_eq!(
            document_local_sha256(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &ir,
                ir.source.as_ref(),
                "absent",
                source_image
            )
            .unwrap(),
            cloned_local_digest(&ir, "absent", source_image)
        );
    }

    #[test]
    fn document_local_sha256_ignores_retained_bytes() {
        let source_image = "synthetic:file:source-image#0";
        let (ir, _source_fidelity) = local_digest_fixture();
        let hash = document_local_sha256(
            &cadmpeg_core::decode::test_support::service_decode_context(),
            &ir,
            ir.source.as_ref(),
            "synthetic",
            source_image,
        )
        .unwrap();

        let (repacked, _source_fidelity) =
            local_digest_fixture_with_source_image(UnknownRecord::retained(
                UnknownId::mint(source_image).expect("valid identity"),
                4,
                vec![9],
                vec![ir.geometry.bodies[0].id.as_str().to_owned()],
            ));
        assert_eq!(
            document_local_sha256(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &repacked,
                repacked.source.as_ref(),
                "synthetic",
                source_image
            )
            .unwrap(),
            hash
        );
    }
    #[test]
    fn procedural_digest_indexes_thousands_of_owners_with_linear_work() {
        use crate::geometry::procedural_curves::ProceduralCurveDefinition;
        use crate::geometry::{Curve, CurveGeometry, ProceduralCurve};
        use crate::ids::{CurveId, ProceduralCurveId};
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let mut ir = CadIr::empty();
        for index in 0..4096 {
            let construction =
                ProceduralCurveId::mint(format!("test:model:construction#{index:04}")).unwrap();
            ir.geometry.curves.push(Curve {
                parameter_range: None,
                id: CurveId::mint(format!("test:model:curve#{index:04}")).unwrap(),
                geometry: CurveGeometry::Procedural {
                    construction: construction.clone(),
                    cache: None,
                },
                source_object: None,
                native_ref: None,
            });
            ir.geometry.procedural_curves.push(ProceduralCurve::new(
                construction,
                ProceduralCurveDefinition::Exact { cache: None },
            ));
        }
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let digest =
            document_local_sha256(&ctx, &ir, None, "test", "test:model:unknown#source").unwrap();
        ctx.finish_session().unwrap();
        ir.geometry.curves.reverse();
        ir.geometry.procedural_curves.reverse();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(
            digest,
            document_local_sha256(&ctx, &ir, None, "test", "test:model:unknown#source").unwrap()
        );
        ctx.finish_session().unwrap();
    }

    #[test]
    fn procedural_digest_still_rejects_orphan_and_ambiguous_owners() {
        use crate::geometry::procedural_curves::ProceduralCurveDefinition;
        use crate::geometry::{Curve, CurveGeometry, ProceduralCurve};
        use crate::ids::{CurveId, ProceduralCurveId};
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        for count in [0, 2, 3] {
            let mut ir = CadIr::empty();
            let construction = ProceduralCurveId::mint("test:model:construction#one").unwrap();
            ir.geometry.procedural_curves.push(ProceduralCurve::new(
                construction.clone(),
                ProceduralCurveDefinition::Exact { cache: None },
            ));
            for index in 0..count {
                ir.geometry.curves.push(Curve {
                    parameter_range: None,
                    id: CurveId::mint(format!("test:model:curve#{index}")).unwrap(),
                    geometry: CurveGeometry::Procedural {
                        construction: construction.clone(),
                        cache: None,
                    },
                    source_object: None,
                    native_ref: None,
                });
            }
            let arena = DecodeArena::new();
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
            assert!(
                matches!(document_local_sha256(&ctx, &ir, None, "test", "test:model:unknown#source"), Err(CodecError::Malformed(message)) if message.contains("no unique owning curve"))
            );
        }
    }

    #[test]
    fn document_digest_refuses_at_the_depth_limit() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let ir = pinned_document_with_source();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = document_local_sha256(
            &ctx,
            &ir,
            ir.source.as_ref(),
            "pin",
            "pin:test:source-image#0",
        )
        .unwrap_err();
        let CodecError::ResourceLimit(original) = error else {
            panic!("digest refusal must stay outside serde");
        };
        assert_eq!(original.dimension, ResourceDimension::RecursionDepth);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
        );
    }

    #[test]
    fn pretty_format_compact_json_matches_canonical_pretty_printer() {
        let samples = [
            serde_json::json!({
                "id": "test:scope:kind#0",
                "empty_obj": {},
                "empty_arr": [],
                "neg_zero": -0.0,
                "neg_zero_str": "-0.0",
                "neg_zero_arr": [-0.0, -0.05, 0.0, -0.0],
                "escaped": "a\"b\\c\n\t\"{}",
                "nested": { "k": [1, true, null, { "x": -0.0 }] }
            }),
            serde_json::json!({
                "id": "test:scope:kind#1",
                "payload": [[-0.0], [[[]]], {"a": {"b": {}}}]
            }),
        ];
        for sample in &samples {
            let compact = serde_json::to_string(sample).unwrap();
            let mut formatted = String::new();
            let max_depth = super::pretty_format_compact_json(&compact, 0, &mut formatted);
            assert!(max_depth >= 2);
            let canonical = crate::hash::finite_json::to_canonical_json_string(sample).unwrap();
            assert_eq!(formatted, canonical, "mismatch for {compact}");
        }
    }

    #[test]
    fn parallel_digest_native_arena_matches_canonical_pretty_json() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut ir = CadIr::empty();
        let mut records = Vec::with_capacity(600);
        for i in 0..600_usize {
            let id = format!("test:scope:item#{i:04}");
            let json = format!("{{\"id\":\"{id}\",\"seq\":{i},\"zero\":-0.0}}");
            records.push(NativeRecord::from_canonical_flat(&ctx, &id, json, 1).unwrap());
        }
        ir.native
            .namespace_mut(crate::codec_id!("test"))
            .set_arena_records("items", records)
            .unwrap();
        ir.set_native_unknowns(&ctx, "test", &[]).unwrap();
        ir.finalize();
        let digest_fast =
            document_local_sha256(&ctx, &ir, None, "test", "test:file:source#0").unwrap();
        assert_eq!(
            digest_fast,
            cloned_local_digest(&ir, "test", "test:file:source#0")
        );
    }
}

#[cfg(test)]
mod charge_tests;
