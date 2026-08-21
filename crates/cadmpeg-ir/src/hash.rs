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
pub fn document_local_sha256(ir: &CadIr, format: &str, source_image_id: &str) -> String {
    document_local_sha256_with_charge(ir, format, source_image_id, |_| {
        Ok::<(), std::convert::Infallible>(())
    })
    .expect("canonical JSON serialization")
}

/// Returns the machine-local document digest while charging each canonical
/// JSON byte through `charge`.
///
/// The charged form keeps the exact normalization and byte stream of
/// [`document_local_sha256`]. A decoder can therefore apply its work budget
/// to the real digest cost and refuse before an oversized document spends the
/// remaining budget on an unbounded whole-document walk.
pub fn document_local_sha256_with_charge<E>(
    ir: &CadIr,
    format: &str,
    source_image_id: &str,
    charge: impl FnMut(u64) -> Result<(), E>,
) -> Result<String, E> {
    let unknowns = reduced_unknowns(ir, format, source_image_id);
    let document = NormalizedDocument {
        ir_version: ir.ir_version(),
        source: ir.source.as_ref().map(|source| {
            let mut source = source.clone();
            source.attributes.remove("document_local_sha256");
            source
        }),
        units: &ir.units,
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
    let serialized = serde_json::to_writer_pretty(&mut writer, &document);
    if let Some(error) = writer.get_mut().error.take() {
        return Err(error);
    }
    serialized.expect("canonical JSON serialization");
    if writer.flush().is_err() {
        if let Some(error) = writer.get_mut().error.take() {
            return Err(error);
        }
        panic!("a digest accepts every byte");
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
fn reduced_unknowns(ir: &CadIr, format: &str, source_image_id: &str) -> Vec<NativeRecord> {
    let mut unreadable = false;
    let mut projected = NativeNamespace::default();
    projected
        .set_arena_from(
            "unknowns",
            ir.native_unknowns_iter(format)
                .map_while(|record| record.inspect_err(|_| unreadable = true).ok())
                .filter(|record| record.id.0 != source_image_id),
        )
        .expect("unknown records serialize");
    if unreadable {
        // Unreadable arenas reduce to empty.
        return Vec::new();
    }
    projected
        .arenas
        .remove("unknowns")
        .expect("the unknown arena was just set")
}

/// A document as the semantic digest sees it.
///
/// Mirrors [`CadIr`]'s serialized shape field for field, borrowing what it can.
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

    use super::{
        canonical_json_sha256, document_local_sha256, document_local_sha256_with_charge, sha256_hex,
    };
    use crate::document::CadIr;
    use crate::examples::unit_cube;
    use crate::ids::UnknownId;
    use crate::native::{Native, NativeRecord};
    use crate::units::Units;
    use crate::unknown::UnknownRecord;

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

    /// Pretty-printed `NativeRecord` bytes covered by the document digest.
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

    /// Same record indented inside namespace and arena, as hashing sees it.
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

    /// Digest over the arena above; formatting drift under
    /// `canonical_json_sha256` fails here.
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

    /// Pins both digest entry points over one fixed, platform-independent
    /// document.
    #[test]
    fn pins_document_digests() {
        let ir = pinned_document();
        assert_eq!(
            canonical_json_sha256(&ir),
            "00ac254ce62cf446f1d1dcea56ded050bd9a1ef2a53c846a8e7c588cd99bb071"
        );
        assert_eq!(
            document_local_sha256(&ir, "pin", "pin:source-image#0"),
            "83fa753fb39360b9e51859c9c07ddac6ff23ec17b179fa548cf33c4331170180"
        );
    }

    #[test]
    fn charged_document_digest_preserves_the_uncharged_digest() {
        let ir = pinned_document();
        let expected = document_local_sha256(&ir, "pin", "pin:source-image#0");
        let mut charged = 0;
        let actual = document_local_sha256_with_charge(&ir, "pin", "pin:source-image#0", |bytes| {
            charged += bytes;
            Ok::<(), ()>(())
        })
        .unwrap();

        assert_eq!(actual, expected);
        assert!(charged > 0);
    }

    #[test]
    fn charged_document_digest_propagates_a_work_refusal() {
        let ir = pinned_document();
        let result = document_local_sha256_with_charge(&ir, "pin", "pin:source-image#0", |_| {
            Err::<(), _>("work limit")
        });

        assert!(matches!(result, Err("work limit")));
    }

    /// The pinned document with the source metadata a decoded document carries:
    /// one recorded baseline the digest must drop, and one ordinary attribute it
    /// must keep.
    fn pinned_document_with_source() -> CadIr {
        let mut ir = pinned_document();
        ir.source = Some(crate::document::SourceMeta {
            format: "pin".into(),
            attributes: [
                ("document_local_sha256".to_owned(), "stale".to_owned()),
                ("file_size".to_owned(), "4096".to_owned()),
            ]
            .into_iter()
            .collect(),
        });
        ir
    }

    /// Pins normalization over source metadata: the recorded baseline attribute
    /// is dropped before hashing and every other attribute is kept.
    #[test]
    fn pins_document_digest_over_source_metadata() {
        let ir = pinned_document_with_source();
        assert_eq!(
            document_local_sha256(&ir, "pin", "pin:source-image#0"),
            "3750864814cc4d83c355df4e8c6942c3b7c682dc15c193b5c836540ad8c07d64"
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

    /// Copy the document, finalize order, drop the recorded digest and retained
    /// source image, and hash the serialized string.
    fn cloned_local_digest(ir: &CadIr, format: &str, source_image_id: &str) -> String {
        let mut normalized = ir.clone();
        normalized.finalize();
        normalized.source = ir.source.as_ref().map(|source| {
            let mut source = source.clone();
            source.attributes.remove("document_local_sha256");
            source
        });
        let unknowns = ir
            .native_unknowns(format)
            .unwrap_or_default()
            .into_iter()
            .filter(|record| record.id.0 != source_image_id)
            .collect::<Vec<_>>();
        normalized.set_native_unknowns(format, &unknowns).unwrap();
        crate::hash::sha256_hex(normalized.to_canonical_json().unwrap().as_bytes())
    }

    /// A document with an unordered model, a recorded digest, two native
    /// namespaces, and a retained source image among the unknown records.
    fn local_digest_fixture() -> CadIr {
        let mut ir = unit_cube();
        ir.model.faces.reverse();
        ir.model.surfaces.reverse();
        ir.source = Some(crate::SourceMeta {
            format: "synthetic".into(),
            attributes: [
                ("document_local_sha256".to_owned(), "stale".to_owned()),
                ("active_brep".to_owned(), "body#0".to_owned()),
            ]
            .into_iter()
            .collect(),
        });
        ir.set_native_unknowns_owned(
            "synthetic",
            vec![
                UnknownRecord {
                    id: UnknownId("synthetic:file:source-image#0".into()),
                    offset: 0,
                    byte_len: 3,
                    sha256: "00".into(),
                    data: Some(vec![1, 2, 3]),
                    links: Vec::new(),
                },
                UnknownRecord {
                    id: UnknownId("synthetic:record#1".into()),
                    offset: 8,
                    byte_len: 2,
                    sha256: "11".into(),
                    data: Some(vec![4, 5]),
                    links: vec!["cube:body#0".into()],
                },
            ],
        );
        let namespace = ir.native.namespace_mut("other");
        namespace.version = 3;
        namespace.arenas.insert(
            "records".into(),
            vec![NativeRecord::new("other:record#0", serde_json::Map::new())],
        );
        ir
    }

    #[test]
    fn document_local_sha256_matches_the_cloned_normalization() {
        let ir = local_digest_fixture();
        let source_image = "synthetic:file:source-image#0";
        assert_eq!(
            crate::hash::document_local_sha256(&ir, "synthetic", source_image),
            cloned_local_digest(&ir, "synthetic", source_image)
        );
        assert_eq!(
            crate::hash::document_local_sha256(&ir, "absent", source_image),
            cloned_local_digest(&ir, "absent", source_image)
        );
    }

    #[test]
    fn document_local_sha256_ignores_the_recorded_digest_and_retained_bytes() {
        let source_image = "synthetic:file:source-image#0";
        let ir = local_digest_fixture();
        let hash = crate::hash::document_local_sha256(&ir, "synthetic", source_image);

        let mut recorded = local_digest_fixture();
        recorded
            .source
            .as_mut()
            .unwrap()
            .attributes
            .insert("document_local_sha256".into(), hash.clone());
        assert_eq!(
            crate::hash::document_local_sha256(&recorded, "synthetic", source_image),
            hash
        );

        let mut repacked = local_digest_fixture();
        let mut records = repacked
            .native
            .namespace("synthetic")
            .unwrap()
            .arenas
            .get("unknowns")
            .unwrap()
            .clone();
        records.retain(|record| record.id() != source_image);
        records.push(
            UnknownRecord {
                id: UnknownId(source_image.into()),
                offset: 4,
                byte_len: 1,
                sha256: "22".into(),
                data: Some(vec![9]),
                links: vec!["cube:body#0".into()],
            }
            .into_native_record(),
        );
        repacked
            .native
            .namespace_mut("synthetic")
            .arenas
            .insert("unknowns".into(), records);
        assert_eq!(
            crate::hash::document_local_sha256(&repacked, "synthetic", source_image),
            hash
        );
    }
}
