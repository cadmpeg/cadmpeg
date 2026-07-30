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
/// [`CadIr::to_canonical_json`] would produce.
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
/// so that normalizing a document for hashing does not copy it.
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
    use super::sha256_hex;

    #[test]
    fn encodes_sha256_as_lowercase_hexadecimal() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
