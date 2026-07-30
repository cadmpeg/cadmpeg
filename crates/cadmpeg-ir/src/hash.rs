// SPDX-License-Identifier: Apache-2.0
//! Content hashing helpers shared by codecs.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::document::{CadIr, SortedModel, SourceMeta};
use crate::native::{Native, NativeNamespace, NativeRecord};
use crate::units::{Tolerances, Units};

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
pub fn canonical_json_sha256<T: Serialize>(value: &T) -> String {
    let mut hasher = Sha256::new();
    let mut writer = std::io::BufWriter::new(DigestWriter(&mut hasher));
    serde_json::to_writer_pretty(&mut writer, value).expect("canonical JSON serialization");
    writer.flush().expect("a digest accepts every byte");
    drop(writer);
    encode_hex(&hasher.finalize())
}

/// Returns the semantic digest of `ir` as seen by the `format` codec.
///
/// The digest covers the document in canonical arena order with two
/// normalizations: the recorded `semantic_sha256` attribute is dropped so a
/// document carrying its own digest hashes the same as one that does not, and
/// the `format` unknown arena is reduced to record identities and links with
/// `source_image_id` — the retained copy of the source container — excluded.
/// Retained source bytes therefore never reach the digest, and the digest is
/// stable across a decode that stores it.
pub fn semantic_document_hash(ir: &CadIr, format: &str, source_image_id: &str) -> String {
    let unknowns = ir
        .native_unknowns(format)
        .unwrap_or_default()
        .into_iter()
        .filter(|record| record.id.0 != source_image_id)
        .collect::<Vec<_>>();
    // A scratch namespace reduces the records through the same conversion and
    // ordering a stored arena goes through, which is what keeps the digest
    // equal to the one a normalized copy of the document produces. The typed
    // records are released before serialization so only the reduced arena stays
    // resident.
    let mut projected = NativeNamespace::default();
    projected
        .set_arena("unknowns", &unknowns)
        .expect("unknown records serialize");
    drop(unknowns);
    let unknown_arena = projected
        .arenas
        .get("unknowns")
        .expect("the unknown arena was just set");
    canonical_json_sha256(&NormalizedDocument {
        ir_version: &ir.ir_version,
        source: ir.source.as_ref().map(|source| {
            let mut source = source.clone();
            source.attributes.remove("semantic_sha256");
            source
        }),
        units: &ir.units,
        tolerances: &ir.tolerances,
        model: ir.model.sorted(),
        native: normalized_native(&ir.native, format, unknown_arena),
    })
}

/// A document as the semantic digest sees it.
///
/// Mirrors [`CadIr`]'s serialized shape field for field, borrowing what it can
/// so that normalizing a document for hashing does not copy it. A field added
/// to [`CadIr`] must be added here too; the equivalence test that hashes a
/// normalized copy of a document fails when the two shapes drift.
#[derive(Serialize)]
struct NormalizedDocument<'a> {
    ir_version: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<SourceMeta>,
    units: &'a Units,
    tolerances: &'a Tolerances,
    model: SortedModel<'a>,
    native: NormalizedNative<'a>,
}

/// Native namespaces in canonical order, with one arena substituted.
#[derive(Serialize)]
#[serde(transparent)]
struct NormalizedNative<'a> {
    namespaces: BTreeMap<&'a str, NormalizedNamespace<'a>>,
}

/// One native namespace whose arenas are borrowed in canonical record order.
#[derive(Serialize)]
struct NormalizedNamespace<'a> {
    version: u32,
    arenas: BTreeMap<&'a str, Vec<&'a NativeRecord>>,
}

/// Borrow every native namespace in canonical order, replacing the `format`
/// unknown arena with `unknowns` and creating that namespace when the document
/// has none.
fn normalized_native<'a>(
    native: &'a Native,
    format: &'a str,
    unknowns: &'a [NativeRecord],
) -> NormalizedNative<'a> {
    let mut namespaces = native
        .0
        .iter()
        .map(|(name, namespace)| {
            let arenas = namespace
                .arenas
                .iter()
                .map(|(arena, records)| (arena.as_str(), sorted_records(records)))
                .collect();
            (
                name.as_str(),
                NormalizedNamespace {
                    version: namespace.version,
                    arenas,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let namespace = namespaces
        .entry(format)
        .or_insert_with(|| NormalizedNamespace {
            version: 0,
            arenas: BTreeMap::new(),
        });
    if namespace.version == 0 {
        namespace.version = 1;
    }
    namespace
        .arenas
        .insert("unknowns", unknowns.iter().collect());
    NormalizedNative { namespaces }
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

/// Render a digest as lowercase hexadecimal.
fn encode_hex(digest: &[u8]) -> String {
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{canonical_json_sha256, semantic_document_hash, sha256_hex};
    use crate::document::CadIr;
    use crate::native::{Native, NativeRecord};
    use crate::units::Units;

    #[test]
    fn encodes_sha256_as_lowercase_hexadecimal() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// A record exercising every JSON shape whose rendering could drift:
    /// nested objects and arrays, escaped and non-ASCII strings, and integers,
    /// negative integers, fractions, integral floats, and exponent-form floats.
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
        NativeRecord::new("pin:record#0", fields)
    }

    fn pinned_native() -> Native {
        let mut native = Native::default();
        let namespace = native.namespace_mut("pin");
        namespace.version = 3;
        namespace
            .arenas
            .insert("records".into(), vec![pinned_record()]);
        native
    }

    /// A `NativeRecord` renders `id` first and then its codec-owned fields in
    /// key order, honouring the caller's pretty formatting at every depth. The
    /// bytes below are the ones the document digest covers, so any change to
    /// how a record reaches a serializer changes every stored semantic hash.
    #[test]
    fn pins_pretty_printed_native_record_bytes() {
        let expected = r#"{
  "id": "pin:record#0",
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

    /// The same bytes indented inside the enclosing namespace and arena, which
    /// is how a record actually reaches `to_writer_pretty` during hashing.
    #[test]
    fn pins_pretty_printed_native_arena_bytes() {
        let expected = r#"{
  "pin": {
    "version": 3,
    "arenas": {
      "records": [
        {
          "id": "pin:record#0",
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
  }
}"#;
        assert_eq!(
            serde_json::to_string_pretty(&pinned_native()).unwrap(),
            expected
        );
    }

    /// A digest over the arena above, pinned so that formatting drift anywhere
    /// under `canonical_json_sha256` fails here rather than silently
    /// invalidating every recorded document digest.
    #[test]
    fn pins_native_arena_digest() {
        assert_eq!(
            canonical_json_sha256(&pinned_native()),
            "acc1d88751dcb143ca47618c3f7a8ce14865edff0a26ab95748d2a0314ee8df0"
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
        NativeRecord::new(id, fields)
    }

    fn pinned_document() -> CadIr {
        let mut ir = CadIr::empty(Units::default());
        ir.native = pinned_native();
        let namespace = ir.native.namespace_mut("pin");
        namespace.arenas.insert(
            "unknowns".into(),
            vec![
                pinned_unknown("pin:source-image#0", &[]),
                pinned_unknown("pin:unknown#0", &["pin:record#0", "pin:unknown#1"]),
            ],
        );
        ir.finalize();
        ir
    }

    /// Both digest entry points over one fixed document. `semantic_document_hash`
    /// reduces the named format's unknown arena to identities and links and
    /// drops the retained source image, so it is pinned alongside the plain
    /// document digest.
    #[test]
    fn pins_document_digests() {
        let ir = pinned_document();
        assert_eq!(
            canonical_json_sha256(&ir),
            "460b8354885d6964a39d52a5f783e47e59a7ab650baf1b322ba7e0e6fd8b823b"
        );
        assert_eq!(
            semantic_document_hash(&ir, "pin", "pin:source-image#0"),
            "9c988f2974e114e5868281ca3ee3391715f112dc9f79cf91b339be744f9f9715"
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
        assert_eq!(canonical_json_sha256(&ir), canonical_json_sha256(&reparsed));
        assert_eq!(ir.to_canonical_json().unwrap(), json);
    }
}
