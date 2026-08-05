// SPDX-License-Identifier: Apache-2.0
//! Golden snapshot harness for `inspect` and `decode` over the committed
//! fixtures.
//!
//! `tests/golden/fixtures/*.sldprt` are the frozen inputs.
//! This harness never writes them: a snapshot test can only tell a decoder
//! change apart from an input change while the inputs hold still, so
//! regenerating an input destroys the evidence the snapshot exists to carry.
//! `UPDATE_GOLDEN=1` rewrites `tests/golden/decode/` and
//! `tests/golden/inspect/`, and nothing else.
//!
//! `tests/golden/inspect/` pins the container summary and
//! `tests/golden/decode/` pins the decoded document: the IR, the decode
//! report's losses, and source fidelity. A feature-typing or loss-accounting
//! change moves the decode branch and `inspect` cannot see it, because an
//! inspect summary describes the container, not what was transferred out of it.
//!
//! [`cadmpeg_codec_core::golden`] holds the enumeration, comparison, and
//! reporting shared with every other codec; this module supplies only this
//! codec's branches.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_codec_core::decode::InspectOptions;
use cadmpeg_codec_core::golden::{
    elide_local_digests, snapshot_text, snapshots_agree, Branch, Harness,
};
use cadmpeg_codec_core::CodecError;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};
use cadmpeg_ir::roundtrip::{semantic_roundtrip, verbatim_replay_holds, SemanticOutcome};
use cadmpeg_ir::WritePath;

use super::SldprtCodec;

/// Extension of the committed fixture inputs.
const FIXTURE_EXTENSION: &str = "sldprt";

/// Crate-relative regeneration hint used in every failure message.
const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-sldprt golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), FIXTURE_EXTENSION, REGENERATE)
}

/// The branches this codec pins, in golden-directory order.
fn branches() -> [Branch; 2] {
    [
        Branch::new("inspect", inspect_snapshot),
        Branch::new("decode", decode_snapshot),
    ]
}

/// Serializes one container summary. An inspect error is frozen too: refusing a
/// container is contract-relevant behavior, so this never panics on codec
/// output.
fn inspect_snapshot(bytes: &[u8]) -> String {
    let value =
        match SldprtCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default()) {
            Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect summary"),
            Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
        };
    snapshot_text(&value)
}

/// Serializes one decoded document: the IR, the decode report, and source
/// fidelity. A decode error is frozen too: refusing a document is
/// contract-relevant behavior, so this never panics on codec output.
fn decode_snapshot(bytes: &[u8]) -> String {
    let value =
        match SldprtCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default()) {
            Ok(mut result) => {
                if let Some(source) = result.ir.source.as_mut() {
                    // The `native` lane digests cover retained source bytes and
                    // stay pinned; a `_local_sha256` digest covers decoded
                    // content, so a platform's libm moves it.
                    elide_local_digests(&mut source.attributes);
                }
                serde_json::json!({
                    "ir": serde_json::to_value(&result.ir).expect("serialize ir"),
                    "report": serde_json::to_value(&result.report).expect("serialize report"),
                    "source_fidelity": serde_json::to_value(&result.source_fidelity)
                        .expect("serialize source_fidelity"),
                })
            }
            Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
        };
    snapshot_text(&value)
}

/// The key of a `SolidWorks` identifier: everything after its last `#`.
fn identifier_key(id: &str) -> Option<&str> {
    id.starts_with("sldprt:")
        .then(|| id.rsplit_once('#'))
        .flatten()
        .map(|(_, key)| key)
}

/// Splits an identifier key into its head component and the rest.
fn split_key(key: &str) -> (&str, Option<&str>) {
    key.split_once(':')
        .map_or((key, None), |(head, rest)| (head, Some(rest)))
}

/// The identifier scopes whose key head is a container section ordinal.
///
/// `container::Section::ordinal` supplies the head for every record read out of
/// a named container section, and these are the scopes that carry one. The
/// `brep` and `appearance` scopes are deliberately absent: their heads are
/// Parasolid entity and attribute tags, which live inside a payload the writer
/// replays unchanged, so they must compare exactly.
const SECTION_SCOPED_IDENTIFIERS: [&str; 6] = [
    "sldprt:displaylist:",
    "sldprt:feature-input:",
    "sldprt:file:",
    "sldprt:history:",
    "sldprt:metadata:",
    "sldprt:model:",
];

/// Every container byte position one identifier carries, as
/// `(family, position)` pairs sharing a rank space.
///
/// This codec mints two of them, and only two:
///
/// | component | what it locates | minted at |
/// | --- | --- | --- |
/// | key head, in a [`SECTION_SCOPED_IDENTIFIERS`] scope | the marker byte offset of the block the record was read from | `container::Section::ordinal` |
/// | second component of `sldprt:metadata:*` | the record's byte offset inside that block's payload | `metadata::attribute` |
///
/// Every other key component is an index within its record — a configuration
/// index, a feature index, a sketch-entity index — and carries no byte position,
/// so it is compared exactly.
///
/// The second component moves for a reason of its own: a rewritten SW Objects
/// payload omits the `moTransRefPlaneData_c` gap, which no field of the document
/// records. `docs/formats/sldprt-open-items.md` CM-07 holds the byte evidence.
/// Closing CM-07 would let the writer reproduce the payload byte for byte and
/// this component could then compare exactly.
fn byte_positions(id: &str) -> Vec<(String, u64)> {
    let Some(key) = identifier_key(id) else {
        return Vec::new();
    };
    if !SECTION_SCOPED_IDENTIFIERS
        .iter()
        .any(|scope| id.starts_with(scope))
    {
        return Vec::new();
    }
    let (head, rest) = split_key(key);
    let Ok(section) = head.parse::<u64>() else {
        return Vec::new();
    };
    let mut positions = vec![(String::from("section"), section)];
    if id.starts_with("sldprt:metadata:") {
        if let Some(offset) = rest
            .map(split_key)
            .and_then(|(offset, _)| offset.parse().ok())
        {
            positions.push((format!("record@{section}"), offset));
        }
    }
    positions
}

/// Rewrites one identifier, replacing each byte position it carries with that
/// position's rank in `ranks`.
fn normalize_identifier(id: &str, ranks: &BTreeMap<(String, u64), usize>) -> Option<String> {
    let positions = byte_positions(id);
    if positions.is_empty() {
        return None;
    }
    let (prefix, key) = id.rsplit_once('#')?;
    let mut components = key.split(':').map(String::from).collect::<Vec<_>>();
    for (index, position) in positions.iter().enumerate() {
        let rank = ranks.get(position)?;
        *components.get_mut(index)? = format!("<{rank}>");
    }
    Some(format!("{prefix}#{}", components.join(":")))
}

/// Walks a decoded document, applying `visit` to every identifier it carries,
/// as an object key or as a value.
fn walk_identifiers(value: &mut serde_json::Value, visit: &mut impl FnMut(&str) -> Option<String>) {
    match value {
        serde_json::Value::String(text) => {
            if let Some(replacement) = visit(text) {
                *text = replacement;
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                walk_identifiers(item, visit);
            }
        }
        serde_json::Value::Object(fields) => {
            let renames = fields
                .keys()
                .filter_map(|key| visit(key).map(|replacement| (key.clone(), replacement)))
                .collect::<Vec<_>>();
            for (from, to) in renames {
                if let Some(field) = fields.remove(&from) {
                    fields.insert(to, field);
                }
            }
            for (_, field) in fields.iter_mut() {
                walk_identifiers(field, visit);
            }
        }
        _ => {}
    }
}

/// Serializes the neutral document a byte string decodes to, with every
/// container byte position an identifier carries replaced by its rank.
///
/// # Why the identifiers need normalizing at all
///
/// This codec derives an identifier from where in the container it read the
/// record: see [`byte_positions`] for the two components that hold one and where
/// each is minted. Repacking the container moves those bytes without changing
/// what they say — re-deflating one block to five more bytes shifts every later
/// block — so a rewrite that changed nothing still renames the entities. Their
/// *order* survives, and that is what the rank captures: two documents agree
/// here when their records sit in the same relative order, whatever the repack
/// did to the offsets. A record that moved past another, appeared, or vanished
/// still fails.
///
/// # What is compared and what is not
///
/// The neutral model, the units, and the tolerances — the document. Two parts of
/// the decode describe the *container* the writer just rebuilt rather than the
/// document it carries, and neither is a claim about the write:
///
/// - `ir.native`, which retains that container's records. A rewrite adds the
///   `Contents/SolidWorks` document envelope whenever the document names an
///   active configuration and no retained block carries one, because that
///   envelope is where the container states which configuration is active.
/// - `ir.source.attributes`, which counts those records and repeats the envelope
///   fields the writer just materialized.
fn neutral_document(bytes: &[u8]) -> String {
    let value =
        match SldprtCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default()) {
            Ok(result) => {
                let mut document = serde_json::json!({
                    "model": serde_json::to_value(&result.ir.model).expect("serialize model"),
                    "units": serde_json::to_value(result.ir.units).expect("serialize units"),
                    "tolerances": serde_json::to_value(result.ir.tolerances)
                        .expect("serialize tolerances"),
                });
                let mut positions = BTreeSet::new();
                walk_identifiers(&mut document, &mut |id| {
                    positions.extend(byte_positions(id));
                    None
                });
                let mut ranks = BTreeMap::new();
                for (family, position) in positions {
                    let rank = ranks
                        .keys()
                        .filter(|(seen, _): &&(String, u64)| *seen == family)
                        .count();
                    ranks.insert((family, position), rank);
                }
                walk_identifiers(&mut document, &mut |id| normalize_identifier(id, &ranks));
                document
            }
            Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
        };
    snapshot_text(&value)
}

/// [`normalize_identifier`] replaces container byte positions and nothing else.
///
/// A normalization broad enough to absorb a repack would, if it reached one
/// component too far, also absorb a writer that dropped a record or renumbered
/// entities — and [`fixtures_survive_the_semantic_write_path`] would then pass on
/// a document the writer had changed. This pins the boundary: distinct positions
/// take distinct ranks, indices survive untouched, and a scope whose head is a
/// Parasolid tag is left alone.
#[test]
fn normalization_replaces_only_container_positions() {
    let ranks = BTreeMap::from([
        (("section".to_string(), 247_u64), 0_usize),
        (("section".to_string(), 400), 1),
        (("record@247".to_string(), 0), 0),
        (("record@247".to_string(), 269), 1),
    ]);

    // Two sections rank apart.
    assert_ne!(
        normalize_identifier("sldprt:model:feature#247:0", &ranks),
        normalize_identifier("sldprt:model:feature#400:0", &ranks)
    );
    // Two records of one section rank apart.
    assert_ne!(
        normalize_identifier("sldprt:metadata:part_record#247:0", &ranks),
        normalize_identifier("sldprt:metadata:part_record#247:269", &ranks)
    );
    // Indices after the position survive.
    assert_eq!(
        normalize_identifier("sldprt:model:sketch-entity#247:0:0:1", &ranks).as_deref(),
        Some("sldprt:model:sketch-entity#<0>:0:0:1")
    );
    assert_ne!(
        normalize_identifier("sldprt:model:sketch-entity#247:0:0:1", &ranks),
        normalize_identifier("sldprt:model:sketch-entity#247:0:0:2", &ranks)
    );
    // A Parasolid entity tag is not a container position.
    assert_eq!(normalize_identifier("sldprt:brep:face#247", &ranks), None);
    assert_eq!(
        normalize_identifier("sldprt:appearance:entity53#247", &ranks),
        None
    );
    // A metadata record offset is a position; the second component of any other
    // scope is an index and stays exact.
    assert_eq!(
        normalize_identifier("sldprt:feature-input:class#247:106", &ranks).as_deref(),
        Some("sldprt:feature-input:class#<0>:106")
    );
}

/// Every committed golden still matches what the codec produces.
#[test]
fn golden_snapshots_hold() {
    harness().check(&branches());
}

/// Putting the same bytes through a branch twice produces identical text.
#[test]
fn golden_output_is_deterministic() {
    harness().check_determinism(&branches());
}

/// Every fixture survives a decode and comes back byte for byte through the
/// verbatim-replay path.
///
/// This covers container fidelity and nothing about the writer: on this path the
/// encoder copies the retained source image and no writer code runs.
/// [`cadmpeg_ir::roundtrip::verbatim_replay_holds`] asserts the path as well as
/// the bytes, so the day a fixture stops replaying, this fails rather than
/// quietly changing what it measures.
///
/// The comparison target is the fixture on disk. A stored copy of each expected
/// output would be byte-identical to its own input and would read as writer
/// evidence while carrying none.
#[test]
fn fixtures_replay_verbatim() {
    for (name, bytes) in harness().fixture_inputs() {
        verbatim_replay_holds(&SldprtCodec, &name, &bytes);
    }
}

/// The semantic write path either reproduces a fixture's document or declares
/// the edit it cannot make.
///
/// [`cadmpeg_ir::roundtrip::semantic_roundtrip`] removes the
/// `document_local_sha256` baseline first, so the encoder cannot show the
/// retained image is still current and must run the writer.
///
/// Two outcomes are contract-conforming and both are asserted. A write must
/// decode back to the same document — see [`neutral_document`] for the one
/// normalization that comparison applies and why. A byte comparison is not the
/// contract here: the writer repacks the container, and a stored byte golden
/// would pin one packing forever.
///
/// A refusal must be `NotImplemented`, which names a capability this codec has
/// not built. Any other refusal means the writer misread or corrupted records it
/// retained itself, which is a defect rather than a gap. Which fixtures land in
/// which arm is not pinned here; pinning it would freeze today's gaps as the
/// specification.
#[test]
fn fixtures_survive_the_semantic_write_path() {
    let mut written_count = 0usize;
    for (name, bytes) in harness().fixture_inputs() {
        let expected = neutral_document(&bytes);
        semantic_roundtrip(&SldprtCodec, &name, &bytes, |outcome| match outcome {
            SemanticOutcome::Written { report, bytes, .. } => {
                written_count += 1;
                assert_eq!(
                    report.write_path,
                    WritePath::Patched,
                    "fixture `{name}`: retained records fed the write, so it patched rather than synthesized"
                );
                if let Err(mismatch) = snapshots_agree(&expected, &neutral_document(bytes)) {
                    panic!("fixture `{name}`: the semantically written container decodes to a different document: {mismatch}");
                }
            }
            SemanticOutcome::Refused { error } => {
                assert!(
                matches!(error, CodecError::NotImplemented(_)),
                "fixture `{name}`: the semantic writer may decline an edit it has not built, but this \
                 refusal reports a defect in what it already retains: {error}"
            );
            }
        });
    }
    assert!(
        written_count > 0,
        "no fixture reached the semantic writer, so this test exercised nothing; \
         add a fixture the writer can write, or this lane is dead"
    );
}
