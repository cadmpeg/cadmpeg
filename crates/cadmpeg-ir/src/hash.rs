// SPDX-License-Identifier: Apache-2.0
//! Content hashing helpers shared by codecs.

use std::collections::BTreeMap;
use std::io::Write as _;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::document::{CadIr, SortedModel, SourceMeta};
use crate::native::{arena_from, Native, NativeConvertError, NativeRecord};
use crate::units::{CanonicalUnitsWire, Tolerances};

mod finite_json;

use finite_json::{FiniteGuard, FiniteSerializer};

/// Returns the lowercase hexadecimal SHA-256 digest of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    encode_hex(&Sha256::digest(bytes))
}

/// Returns the lowercase hexadecimal SHA-256 digest of `value`'s canonical
/// pretty JSON.
///
/// The JSON is streamed into the digest, so hashing a document costs a fixed
/// buffer rather than a serialized copy of it. The bytes hashed are the ones
/// `serde_json::to_string_pretty` produces, which is what
/// [`CadIr::to_canonical_json`] returns.
pub fn canonical_json_sha256<T: Serialize>(value: &T) -> Result<String, DigestError> {
    let mut hasher = Sha256::new();
    let mut writer = std::io::BufWriter::new(DigestWriter(&mut hasher));
    write_canonical_json(&mut writer, value)?;
    writer.flush().map_err(DigestError::Write)?;
    drop(writer);
    Ok(encode_hex(&hasher.finalize()))
}

/// Writes `value` as canonical pretty JSON, refusing a non-finite float.
///
/// The bytes are the ones `serde_json::to_writer_pretty` produces. The float
/// refusal is the adapter's, not `serde_json`'s: `serde_json` writes a
/// non-finite float as `null`.
fn write_canonical_json<W: std::io::Write, T: Serialize + ?Sized>(
    writer: W,
    value: &T,
) -> Result<(), DigestError> {
    let guard = FiniteGuard::new();
    let mut json = serde_json::Serializer::pretty(writer);
    match value.serialize(FiniteSerializer::new(&mut json, &guard)) {
        Ok(()) => Ok(()),
        Err(error) => match guard.refused() {
            Some(value) => Err(DigestError::NonFinite { value }),
            None => Err(DigestError::Serialize(error)),
        },
    }
}

/// A digest could not be computed.
///
/// The IR carries raw `f64` in places. `serde_json` writes a non-finite one as
/// `null`, so the digest serializes through an adapter that refuses it instead:
/// see [`NonFinite`](DigestError::NonFinite).
#[derive(Debug, thiserror::Error)]
pub enum DigestError {
    /// A record the unknown arena cannot state.
    #[error(transparent)]
    Record(#[from] NativeConvertError),
    /// The document does not serialize as canonical JSON.
    #[error("canonical JSON serialization: {0}")]
    Serialize(serde_json::Error),
    /// A float on the document is not finite, so canonical JSON cannot state it.
    #[error("canonical JSON holds no non-finite float: {value}")]
    NonFinite {
        /// The refused float.
        value: f64,
    },
    /// The digest sink refused a byte.
    #[error("digest sink: {0}")]
    Write(std::io::Error),
}

impl From<DigestError> for cadmpeg_core::CodecError {
    fn from(error: DigestError) -> Self {
        Self::Malformed(error.to_string())
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
/// excluded. Retained source bytes never reach the digest.
///
/// Bitwise SHA-256 for the write path's edit oracle. Not portable across
/// platforms (libm last-place drift) and not tolerance-aware (tolerant equality
/// is not transitive). Attributes with these properties use
/// [`cadmpeg_ir::compare::LOCAL_DIGEST_SUFFIX`]; see
/// [`crate::document::SourceMeta`].
pub fn document_local_sha256(
    ir: &CadIr,
    format: &str,
    source_image_id: &str,
) -> Result<String, DigestError> {
    document_local_sha256_with_source_and_charge(
        ir,
        ir.source.as_ref(),
        format,
        source_image_id,
        |_| Ok::<(), DigestError>(()),
    )
}

/// Returns the machine-local content digest of `ir` with source metadata that
/// its producer has not assigned to the document yet.
///
/// The digest covers `source` without its own `document_local_sha256`
/// attribute. All other normalization is identical to
/// [`document_local_sha256`].
pub fn document_local_sha256_with_source(
    ir: &CadIr,
    source: &SourceMeta,
    format: &str,
    source_image_id: &str,
) -> Result<String, DigestError> {
    document_local_sha256_with_source_and_charge(ir, Some(source), format, source_image_id, |_| {
        Ok::<(), DigestError>(())
    })
}

/// Returns the machine-local document digest while charging each canonical
/// JSON byte through `charge`.
///
/// The charged form keeps the exact normalization and byte stream of
/// [`document_local_sha256`]. A decoder can therefore apply its work budget
/// to the real digest cost and refuse before an oversized document spends the
/// remaining budget on an unbounded whole-document walk.
pub fn document_local_sha256_with_charge<E: From<DigestError>>(
    ir: &CadIr,
    format: &str,
    source_image_id: &str,
    charge: impl FnMut(u64) -> Result<(), E>,
) -> Result<String, E> {
    document_local_sha256_with_source_and_charge(
        ir,
        ir.source.as_ref(),
        format,
        source_image_id,
        charge,
    )
}

fn document_local_sha256_with_source_and_charge<E: From<DigestError>>(
    ir: &CadIr,
    source: Option<&SourceMeta>,
    format: &str,
    source_image_id: &str,
    charge: impl FnMut(u64) -> Result<(), E>,
) -> Result<String, E> {
    let unknowns = reduced_unknowns(ir, format, source_image_id).map_err(DigestError::from)?;
    let document = NormalizedDocument {
        ir_version: ir.ir_version(),
        source: source.map(|source| {
            let mut source = source.clone();
            source.attributes.remove(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE);
            source
        }),
        units: CanonicalUnitsWire::default(),
        tolerances: &ir.tolerances,
        model: ir.model.sorted(),
        native: normalized_native(&ir.native, format, &unknowns),
    };
    let mut hasher = Sha256::new();
    let mut writer = std::io::BufWriter::with_capacity(
        1024 * 1024,
        ChargingDigestWriter {
            hasher: &mut hasher,
            charge,
            error: None,
        },
    );
    let serialized = write_canonical_json(&mut writer, &document);
    if let Some(error) = writer.get_mut().error.take() {
        return Err(error);
    }
    serialized.map_err(E::from)?;
    if let Err(error) = writer.flush() {
        if let Some(charged) = writer.get_mut().error.take() {
            return Err(charged);
        }
        return Err(E::from(DigestError::Write(error)));
    }
    drop(writer);
    Ok(encode_hex(&hasher.finalize()))
}

/// Reduce the `format` unknown arena to record identities and links, dropping
/// `source_image_id`, in canonical order.
///
/// Each record is deserialized, filtered, and converted back before the next is
/// read, so the retained population is never resident in typed and reduced form
/// at once.
///
/// A record the arena cannot state is the record's own read error, not an
/// empty arena: a document whose unknowns are unreadable has no digest, and
/// must not collide with a document that carries no unknowns at all.
fn reduced_unknowns(
    ir: &CadIr,
    format: &str,
    source_image_id: &str,
) -> Result<Vec<NativeRecord>, NativeConvertError> {
    arena_from(
        ir.native_unknowns_iter(format)
            .filter(|record| match record {
                Ok(record) => record.id.as_str() != source_image_id,
                Err(_) => true,
            }),
    )
}

/// A document as the semantic digest sees it.
///
/// Mirrors [`CadIr`]'s serialized shape field for field, borrowing what it can.
#[derive(Serialize)]
struct NormalizedDocument<'a> {
    ir_version: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<SourceMeta>,
    units: CanonicalUnitsWire,
    tolerances: &'a Tolerances,
    model: SortedModel<'a>,
    native: BTreeMap<&'a str, BTreeMap<&'a str, Vec<&'a NativeRecord>>>,
}

/// Borrow every native namespace in canonical order, replacing the `format`
/// unknown arena with `unknowns` and creating that namespace when the document
/// has none.
fn normalized_native<'a>(
    native: &'a Native,
    format: &'a str,
    unknowns: &'a [NativeRecord],
) -> BTreeMap<&'a str, BTreeMap<&'a str, Vec<&'a NativeRecord>>> {
    let mut namespaces = native
        .0
        .iter()
        .map(|(name, namespace)| {
            let arenas = namespace
                .arenas()
                .iter()
                .map(|(arena, records)| (arena.as_str(), sorted_records(records)))
                .collect();
            (name.as_str(), arenas)
        })
        .collect::<BTreeMap<_, BTreeMap<_, _>>>();
    namespaces
        .entry(format)
        .or_default()
        .insert("unknowns", unknowns.iter().collect());
    namespaces
}

/// Borrow `records` in canonical identity order.
fn sorted_records(records: &[NativeRecord]) -> Vec<&NativeRecord> {
    let mut refs = records.iter().collect::<Vec<_>>();
    refs.sort_by(|left, right| left.id().cmp(right.id()));
    refs
}

/// A sink that feeds every written byte to a digest.
struct DigestWriter<'a>(&'a mut Sha256);

impl std::io::Write for DigestWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct ChargingDigestWriter<'a, F, E> {
    hasher: &'a mut Sha256,
    charge: F,
    error: Option<E>,
}

impl<F, E> std::io::Write for ChargingDigestWriter<'_, F, E>
where
    F: FnMut(u64) -> Result<(), E>,
{
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Err(error) = (self.charge)(buf.len() as u64) {
            self.error = Some(error);
            return Err(std::io::Error::other("digest work charge rejected"));
        }
        self.hasher.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Lowercase hexadecimal digits, indexed by nibble.
const HEX_DIGITS: [char; 16] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f',
];

/// Render a digest as lowercase hexadecimal.
///
/// A nibble indexes the digit table, so this writes without a formatter and
/// has no failure to report.
fn encode_hex(digest: &[u8]) -> String {
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(HEX_DIGITS[usize::from(byte >> 4)]);
        encoded.push(HEX_DIGITS[usize::from(byte & 0x0f)]);
    }
    encoded
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{
        canonical_json_sha256, document_local_sha256, document_local_sha256_with_charge,
        sha256_hex, DigestError, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE,
    };
    use crate::document::CadIr;
    use crate::examples::unit_cube;
    use crate::ids::UnknownId;
    use crate::native::{Native, NativeRecord};
    use crate::unknown::UnknownRecord;
    use cadmpeg_core::CodecError;

    #[test]
    fn a_finite_float_digests() {
        assert!(canonical_json_sha256(&1.0f64).is_ok());
        assert!(canonical_json_sha256(&vec![1.0f64, -2.5]).is_ok());
    }

    #[test]
    fn a_non_finite_float_has_no_digest() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let Err(DigestError::NonFinite { value: refused }) = canonical_json_sha256(&value)
            else {
                panic!("a non-finite float must have no canonical JSON");
            };
            assert_eq!(refused.is_nan(), value.is_nan());
            assert_eq!(refused.is_infinite(), value.is_infinite());
        }
    }

    #[test]
    fn a_nested_non_finite_float_has_no_digest() {
        let nested = serde_json::json!({ "outer": [{ "inner": 1.0 }] });
        assert!(canonical_json_sha256(&nested).is_ok());
        let nested = vec![Some(vec![(1.0f64, f64::NAN)])];
        assert!(matches!(
            canonical_json_sha256(&nested),
            Err(DigestError::NonFinite { .. })
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
        NativeRecord::new("pin:test:record#0", fields).expect("valid native identity")
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
            canonical_json_sha256(&pinned_native()).unwrap(),
            "7249c236a39ac27b8614a9ef11d6b1e1c416e1242d909e6d0e94a96c1d6507d4"
        );
    }

    /// A record carrying the members the unknown reduction keeps (`id`,
    /// `links`) alongside ones it drops.
    fn pinned_unknown(id: &str, links: &[&str]) -> NativeRecord {
        let serde_json::Value::Object(fields) = serde_json::json!({
            "links": links,
            "offset": 4096,
            "byte_len": 12,
            "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
            "data": "AQID"
        }) else {
            panic!("the pinned unknown literal is a JSON object");
        };
        NativeRecord::new(id, fields).expect("valid native identity")
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
            vec![NativeRecord::new("pin:model:record#0", fields).expect("valid native identity")],
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
        let absent = document_local_sha256(&pinned_document(), "pin", source_image).unwrap();

        let mut readable_fields = serde_json::Map::new();
        readable_fields.insert("links".into(), serde_json::json!([]));
        let readable = document_local_sha256(
            &pinned_document_with_unknown(readable_fields),
            "pin",
            source_image,
        )
        .unwrap();
        assert_ne!(readable, absent);

        let mut unreadable_fields = serde_json::Map::new();
        unreadable_fields.insert("links".into(), serde_json::json!(7));
        let unreadable = pinned_document_with_unknown(unreadable_fields);
        assert!(document_local_sha256(&unreadable, "pin", source_image).is_err());
        assert!(document_local_sha256_with_charge::<CodecError>(
            &unreadable,
            "pin",
            source_image,
            |_| { Ok(()) }
        )
        .is_err());
    }

    /// Pins both digest entry points over one fixed, platform-independent
    /// document.
    #[test]
    fn pins_document_digests() {
        let ir = pinned_document();
        assert_eq!(
            canonical_json_sha256(&ir).unwrap(),
            "d1ba8ac967bf02f410e362b0ba5cdefaa410bbbab7c21d4180443b16906ff487"
        );
        assert_eq!(
            document_local_sha256(&ir, "pin", "pin:test:source-image#0").unwrap(),
            "ab55b3269d93d9cf9a76ba6b0ffe0166598d143349b52f545569bfe77768366b"
        );
    }

    #[test]
    fn charged_document_digest_preserves_the_uncharged_digest() {
        let ir = pinned_document();
        let expected = document_local_sha256(&ir, "pin", "pin:test:source-image#0").unwrap();
        let mut charged = 0;
        let actual = document_local_sha256_with_charge::<DigestError>(
            &ir,
            "pin",
            "pin:test:source-image#0",
            |bytes| {
                charged += bytes;
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(actual, expected);
        assert!(charged > 0);
    }

    #[test]
    fn charged_document_digest_propagates_a_work_refusal() {
        let ir = pinned_document();
        let result =
            document_local_sha256_with_charge(&ir, "pin", "pin:test:source-image#0", |_| {
                Err::<(), _>(CodecError::NotImplemented("work limit".into()))
            });

        assert!(matches!(
            result,
            Err(CodecError::NotImplemented(message)) if message == "work limit"
        ));
    }

    /// The pinned document with the source metadata a decoded document carries:
    /// one recorded baseline the digest must drop, and one ordinary attribute it
    /// must keep.
    fn pinned_document_with_source() -> CadIr {
        let mut ir = pinned_document();
        ir.source = Some(crate::document::SourceMeta::classified(
            cadmpeg_core::dialect::DialectLayers::of(
                cadmpeg_core::dialect::DialectMatch::admitted(
                    cadmpeg_core::dialect::DialectId::pinned("pin:test"),
                ),
            ),
            [
                (
                    DOCUMENT_LOCAL_DIGEST_ATTRIBUTE.to_owned(),
                    "stale".to_owned(),
                ),
                ("file_size".to_owned(), "4096".to_owned()),
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
    ///       "classification": "classified",
    ///       "dialects": {
    ///         "primary": {
    ///           "dialect": "pin:test",
    ///           "admission": "admitted"
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
            "28da9611ed9814d3fcd50f95d90199f4dfa0bf578a430e8356d735a428539cb8"
        );
        assert_eq!(
            document_local_sha256(&ir, "pin", "pin:test:source-image#0").unwrap(),
            independently_normalized
        );
    }

    #[test]
    fn local_source_digest_matches_an_assigned_source_without_cloning_the_document() {
        let mut ir = pinned_document_with_source();
        let source = ir.source.take().expect("fixture carries source metadata");

        assert_eq!(
            crate::hash::document_local_sha256_with_source(
                &ir,
                &source,
                "pin",
                "pin:test:source-image#0",
            )
            .unwrap(),
            document_local_sha256(
                &pinned_document_with_source(),
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
            canonical_json_sha256(&ir).unwrap(),
            canonical_json_sha256(&reparsed).unwrap()
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
            .unwrap_or_default()
            .into_iter()
            .filter(|record| record.id.as_str() != source_image_id)
            .collect::<Vec<_>>();
        normalized.set_native_unknowns(format, &unknowns).unwrap();
        crate::hash::sha256_hex(normalized.to_canonical_json().unwrap().as_bytes())
    }

    /// A document with an unordered model, a recorded digest, two native
    /// namespaces, and a retained source image among the unknown records.
    fn local_digest_fixture_with_source_image(
        source_image: UnknownRecord,
    ) -> (CadIr, crate::SourceFidelity) {
        let mut ir = unit_cube();
        ir.model.faces.reverse();
        ir.model.surfaces.reverse();
        ir.source = Some(crate::SourceMeta::classified(
            cadmpeg_core::dialect::DialectLayers::of(
                cadmpeg_core::dialect::DialectMatch::admitted(
                    cadmpeg_core::dialect::DialectId::pinned("synthetic:test"),
                ),
            ),
            [
                (
                    DOCUMENT_LOCAL_DIGEST_ATTRIBUTE.to_owned(),
                    "stale".to_owned(),
                ),
                ("active_brep".to_owned(), "body#0".to_owned()),
            ]
            .into_iter()
            .collect(),
        ));
        let mut source_fidelity = crate::SourceFidelity::default();
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
                        vec!["cube:body#0".into()],
                    ),
                ],
            )
            .unwrap();
        let namespace = ir.native.namespace_mut("other");
        namespace.arenas_mut().insert(
            "records".into(),
            vec![
                NativeRecord::new("other:test:record#0", serde_json::Map::new())
                    .expect("valid native identity"),
            ],
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
            crate::hash::document_local_sha256(&ir, "synthetic", source_image).unwrap(),
            cloned_local_digest(&ir, "synthetic", source_image)
        );
        assert_eq!(
            crate::hash::document_local_sha256(&ir, "absent", source_image).unwrap(),
            cloned_local_digest(&ir, "absent", source_image)
        );
    }

    #[test]
    fn document_local_sha256_ignores_the_recorded_digest_and_retained_bytes() {
        let source_image = "synthetic:file:source-image#0";
        let (ir, _source_fidelity) = local_digest_fixture();
        let hash = crate::hash::document_local_sha256(&ir, "synthetic", source_image).unwrap();

        let (mut recorded, _source_fidelity) = local_digest_fixture();
        recorded
            .source
            .as_mut()
            .unwrap()
            .attributes
            .insert(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE.into(), hash.clone());
        assert_eq!(
            crate::hash::document_local_sha256(&recorded, "synthetic", source_image).unwrap(),
            hash
        );

        let (repacked, _source_fidelity) =
            local_digest_fixture_with_source_image(UnknownRecord::retained(
                UnknownId::mint(source_image).expect("valid identity"),
                4,
                vec![9],
                vec!["cube:body#0".into()],
            ));
        assert_eq!(
            crate::hash::document_local_sha256(&repacked, "synthetic", source_image).unwrap(),
            hash
        );
    }
}
