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

use finite_json::admission::CanonicalAdmission;
use finite_json::{write_canonical_json, CanonicalJsonError};

/// Bytes gathered before the hasher sees them; the pretty JSON writer emits one
/// short write per token.
const DIGEST_BUFFER_BYTES: usize = 64 * 1024;

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
    let mut buffered =
        std::io::BufWriter::with_capacity(DIGEST_BUFFER_BYTES, DigestWriter(Sha256::new()));
    write_canonical_json(
        CanonicalAdmission::limited(ctx, "walk document digest"),
        &mut buffered,
        value,
    )?;
    let writer = buffered
        .into_inner()
        .map_err(|error| DigestError::Write(error.into_error()))?;
    Ok(digest::Sha256Digest::from_bytes(writer.0.finalize().into()).into())
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

/// The source-attribute key under which a codec records
/// [`document_local_sha256`].
///
/// This one key gates the whole-document write decision: an encoder that finds
/// the recorded value still equal to a freshly computed one replays its retained
/// bytes, and otherwise runs its writer. Other `_local_sha256` attributes answer
/// narrower questions — which lane changed, and how — so they are not
/// interchangeable with this one and removing them does not move the same
/// branch.
pub const DOCUMENT_LOCAL_DIGEST_ATTRIBUTE: &str = "document_local_sha256";

/// Returns the machine-local content digest of `ir` as seen by the `format`
/// codec, for recording as the `document_local_sha256` source attribute.
///
/// Covers the document in canonical arena order with two normalizations: the
/// recorded `document_local_sha256` attribute is dropped, and the `format`
/// unknown arena is reduced to identities and links with `source_image_id`
/// excluded. Retained source bytes never reach the digest. `source` states
/// the metadata to include, including metadata not assigned to `ir` yet.
/// Normalization, ordering, serialization and hashing use `ctx`.
///
/// Bitwise SHA-256 for the write path's edit oracle. Not portable across
/// platforms (libm last-place drift) and not tolerance-aware (tolerant equality
/// is not transitive). Attributes with these properties use
/// [`crate::compare::LOCAL_DIGEST_SUFFIX`]; see
/// [`crate::document::SourceMeta`].
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
    let document = (|| {
        Ok::<_, CodecError>(NormalizedDocument {
            ir_version: ir.ir_version(),
            fragment: ir.fragment,
            source: source.map(SourceMeta::normalized_digest_copy),
            units: CanonicalUnitsWire::default(),
            tolerances: &ir.tolerances,
            geometry: ir.geometry.sorted(),
            model: ir.model.sorted()?,
            design: ir.design.sorted(),
            process: ir.process.sorted(),
            native: normalized_native(ctx, &ir.native, format, &unknowns, carried),
        })
    })()?;
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
    native: BTreeMap<&'a str, BTreeMap<&'a str, Vec<DigestNativeRecord<'a, 'input>>>>,
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

fn push_indent(out: &mut Vec<u8>, level: usize) {
    const SPACES: &[u8] = b"                                                                ";
    let mut remaining = level * 2;
    while remaining >= SPACES.len() {
        out.extend_from_slice(SPACES);
        remaining -= SPACES.len();
    }
    out.extend_from_slice(&SPACES[..remaining]);
}

/// Format canonical compact JSON text into `serde_json::ser::PrettyFormatter`
/// bytes starting at `base_indent`, normalizing `-0.0` outside strings to
/// `0.0`. Returns the maximum serializer depth reached inside the value
/// (containers plus scalar leaves).
fn pretty_format_compact_json(compact: &str, base_indent: usize, out: &mut Vec<u8>) -> usize {
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
                while i < len {
                    let b = bytes[i];
                    i += 1;
                    if b == b'\\' {
                        if i < len {
                            i += 1;
                        }
                    } else if b == b'"' {
                        break;
                    }
                }
                out.extend_from_slice(&bytes[start..i]);
                max_depth = max_depth.max(depth + 1);
            }
            b'{' => {
                if bytes.get(i + 1) == Some(&b'}') {
                    out.extend_from_slice(b"{}");
                    max_depth = max_depth.max(depth + 1);
                    i += 2;
                } else {
                    depth += 1;
                    max_depth = max_depth.max(depth);
                    out.push(b'{');
                    out.push(b'\n');
                    push_indent(out, base_indent + depth);
                    i += 1;
                }
            }
            b'[' => {
                if bytes.get(i + 1) == Some(&b']') {
                    out.extend_from_slice(b"[]");
                    max_depth = max_depth.max(depth + 1);
                    i += 2;
                } else {
                    depth += 1;
                    max_depth = max_depth.max(depth);
                    out.push(b'[');
                    out.push(b'\n');
                    push_indent(out, base_indent + depth);
                    i += 1;
                }
            }
            b'}' | b']' => {
                let closing = bytes[i];
                depth = depth.max(1) - 1;
                out.push(b'\n');
                push_indent(out, base_indent + depth);
                out.push(closing);
                i += 1;
            }
            b',' => {
                out.push(b',');
                out.push(b'\n');
                push_indent(out, base_indent + depth);
                i += 1;
            }
            b':' => {
                out.extend_from_slice(b": ");
                i += 1;
            }
            b'-' if bytes.get(i..i + 4) == Some(b"-0.0")
                && matches!(bytes.get(i + 4), None | Some(b',' | b'}' | b']')) =>
            {
                out.extend_from_slice(b"0.0");
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
                out.extend_from_slice(&bytes[start..i]);
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
            static PRETTY_SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
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
                let mut guards = Vec::with_capacity(extra);
                for _ in 0..extra {
                    match self.ctx.enter_nested("walk document digest") {
                        Ok(guard) => guards.push(guard),
                        Err(_) => {
                            return Err(serde::ser::Error::custom(
                                "canonical JSON resource admission refused",
                            ));
                        }
                    }
                }
            }
            let raw = std::str::from_utf8(&buf)
                .map_err(|_| serde::ser::Error::custom("canonical native text is not UTF-8"))?;
            let mut state = serializer.serialize_struct("$serde_json::private::RawValue", 1)?;
            state.serialize_field("$serde_json::private::RawValue", raw)?;
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
) -> BTreeMap<&'a str, BTreeMap<&'a str, Vec<DigestNativeRecord<'a, 'input>>>> {
    let wrap = |records: &'a [NativeRecord]| -> Vec<DigestNativeRecord<'a, 'input>> {
        sorted_records(records)
            .into_iter()
            .map(|record| DigestNativeRecord { ctx, record })
            .collect()
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
            let refs = unknowns
                .iter()
                .map(|record| DigestNativeRecord { ctx, record })
                .collect::<Vec<_>>();
            arenas.insert("unknowns", refs);
        }
        namespaces.insert(name.as_str(), arenas);
    }
    if !namespaces.contains_key(format) {
        let mut arenas = BTreeMap::new();
        let refs = unknowns
            .iter()
            .map(|record| DigestNativeRecord { ctx, record })
            .collect::<Vec<_>>();
        arenas.insert("unknowns", refs);
        namespaces.insert(format, arenas);
    }
    namespaces
}

fn sorted_records(records: &[NativeRecord]) -> Vec<&NativeRecord> {
    let mut refs = records.iter().collect::<Vec<_>>();
    refs.sort_by(|left, right| left.id().cmp(right.id()));
    refs
}

/// A byte sink that feeds a SHA-256 hasher.
struct DigestWriter(Sha256);

impl std::io::Write for DigestWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
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
    use super::{
        canonical_json_sha256, document_local_sha256, sha256_hex, DigestError,
        DOCUMENT_LOCAL_DIGEST_ATTRIBUTE,
    };
    use crate::document::CadIr;
    use crate::examples::unit_cube;
    use crate::ids::UnknownId;
    use crate::native::{Native, NativeRecord};
    use crate::unknown::UnknownRecord;
    use cadmpeg_core::CodecError;

    #[test]
    fn canonical_json_sha256_matches_json_larger_than_the_buffer() {
        let value: Vec<String> = (0..6_000).map(|index| format!("entry-{index}")).collect();
        let json = crate::hash::finite_json::to_canonical_json_string(&value).unwrap();
        assert!(json.len() > super::DIGEST_BUFFER_BYTES);
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
        let namespace = native.namespace_mut("pin");
        namespace
            .arenas_mut()
            .insert("records".into(), vec![pinned_record()]);
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
        let namespace = ir.native.namespace_mut("pin");
        namespace.arenas_mut().insert(
            "unknowns".into(),
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
        ir.native.namespace_mut("pin").arenas_mut().insert(
            "unknowns".into(),
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
            "ed973e3fccbfe465c1ce26ee06372ac3151356fcdaeba850c5bd028009a61e02"
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
            "b05cf37e11d812511ae3830a8f0d138eb69e7d0fbf87b00534f39a79a30722de"
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
    /// one recorded baseline the digest must drop, and one ordinary attribute it
    /// must keep.
    fn pinned_document_with_source() -> CadIr {
        let mut ir = pinned_document();
        ir.source = Some(crate::document::SourceMeta::classified(
            cadmpeg_core::dialect::DialectLayers::of(
                cadmpeg_core::dialect::DialectMatch::admitted(cadmpeg_core::dialect_id!(
                    "pin:test"
                )),
            ),
            [
                (
                    cadmpeg_core::nonblank_const!(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE),
                    "stale".to_owned(),
                ),
                (
                    cadmpeg_core::nonblank_literal!("file_size"),
                    "4096".to_owned(),
                ),
            ]
            .into_iter()
            .collect(),
        ));
        ir
    }

    /// Pins normalization over source metadata: the recorded baseline attribute
    /// is dropped before hashing and every other attribute is kept.
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
            "93ec763bbb89f3a4269601f6ba4102cd6c0bb47cc670fa2fbec1999ebee25888"
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

    /// Copy the document, finalize order, drop the recorded digest and retained
    /// source image, and hash the serialized string.
    fn cloned_local_digest(ir: &CadIr, format: &str, source_image_id: &str) -> String {
        let mut normalized = ir.clone();
        normalized.finalize();
        normalized.source = ir.source.as_ref().map(|source| {
            let mut source = source.clone();
            source.attributes.remove(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE);
            source
        });
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
            [
                (
                    cadmpeg_core::nonblank_const!(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE),
                    "stale".to_owned(),
                ),
                (
                    cadmpeg_core::nonblank_literal!("active_brep"),
                    "body#0".to_owned(),
                ),
            ]
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
        let namespace = ir.native.namespace_mut("other");
        namespace.arenas_mut().insert(
            "records".into(),
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
    fn document_local_sha256_ignores_the_recorded_digest_and_retained_bytes() {
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

        let (mut recorded, _source_fidelity) = local_digest_fixture();
        recorded.source.as_mut().unwrap().attributes.insert(
            cadmpeg_core::nonblank_const!(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE),
            hash.clone(),
        );
        assert_eq!(
            document_local_sha256(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                &recorded,
                recorded.source.as_ref(),
                "synthetic",
                source_image
            )
            .unwrap(),
            hash
        );

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
            let mut formatted = Vec::new();
            let max_depth = super::pretty_format_compact_json(&compact, 0, &mut formatted);
            assert!(max_depth >= 2);
            let canonical = crate::hash::finite_json::to_canonical_json_string(sample).unwrap();
            assert_eq!(
                String::from_utf8(formatted).unwrap(),
                canonical,
                "mismatch for {compact}"
            );
        }
    }
}

#[cfg(test)]
mod charge_tests;
