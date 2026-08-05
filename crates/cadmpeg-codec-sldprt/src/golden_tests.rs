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
use cadmpeg_ir::features::{ExtrudeExtent, FeatureDefinition, Length, Termination};
use cadmpeg_ir::roundtrip::{
    mutation_roundtrip, semantic_roundtrip, verbatim_replay_holds, MutationOutcome, SemanticOutcome,
};
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
            Ok(result) => normalized_document(&result.ir),
            Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
        };
    snapshot_text(&value)
}

/// The model, units, and tolerances with container byte positions rank-normalized.
fn normalized_document(ir: &cadmpeg_ir::CadIr) -> serde_json::Value {
    let mut document = serde_json::json!({
        "model": serde_json::to_value(&ir.model).expect("serialize model"),
        "units": serde_json::to_value(&ir.units).expect("serialize units"),
        "tolerances": serde_json::to_value(ir.tolerances).expect("serialize tolerances"),
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

/// How far the mutation lane moves an extrusion depth, in millimetres.
///
/// Far enough that the tolerant comparator cannot mistake a dropped edit for
/// last-place disagreement.
const MUTATION_MM: f64 = 3.0;

/// Every statement of a one-sided blind extrusion depth in `ir`.
///
/// A blind depth is the plainest dimensional edit this codec has: one number a
/// user types into a feature dialog, carried by the neutral feature arena and by
/// a retained history record at once, which is what makes the write interesting.
///
/// The document states it in two places, and an edit means both: the feature's
/// own definition, and each configuration's evaluated state for that feature.
/// Moving only the first leaves the document self-inconsistent, and the writer
/// then re-derives the configuration state from the feature and the output
/// disagrees with the input for a reason that is not a lost edit.
fn blind_extrude_lengths(ir: &mut cadmpeg_ir::CadIr) -> Vec<&mut Length> {
    fn depth(definition: &mut FeatureDefinition) -> Option<&mut Length> {
        let FeatureDefinition::Extrude {
            extent: ExtrudeExtent::OneSided { side },
            ..
        } = definition
        else {
            return None;
        };
        match &mut side.termination {
            Termination::Blind { length } => Some(length),
            _ => None,
        }
    }
    let features = ir
        .model
        .features
        .iter_mut()
        .filter_map(|feature| depth(&mut feature.definition));
    let states = ir
        .model
        .configurations
        .iter_mut()
        .flat_map(|configuration| configuration.feature_states.values_mut())
        .filter_map(|state| depth(&mut state.definition));
    features.chain(states).collect()
}

/// An edited extrusion depth survives the semantic write path.
///
/// [`fixtures_survive_the_semantic_write_path`] removes the document baseline and
/// leaves the document itself alone. `prepare_features_for_write` then finds the
/// per-lane `sldprt_neutral_feature_local_sha256` baseline still present and still
/// matching and returns on its unchanged-baseline branch, so `sync_neutral_features`
/// — the pass that carries a neutral feature edit into the retained history
/// records — is not reached from the fixture corpus at all. Editing a depth is
/// what reaches it: the neutral feature hash moves, the native history hash does
/// not, and the write runs the synchronization.
///
/// # What this asserts, and what it cannot
///
/// It asserts that the edit comes back: every place the document states the depth
/// states the edited value after the write. It also asserts that no B-rep arena
/// changed size, so a face or edge lost during the rewrite still fails here.
///
/// It does not compare the written document as a whole, because the written
/// B-rep is not the retained one. `retained_partition` replays the source
/// Parasolid payload only while `brep_local_sha256` still matches, and that digest
/// covers the whole `model` rather than the B-rep arenas its name refers to. A
/// depth edit touches no B-rep entity and still moves it, so the writer discards
/// the retained payload and re-authors the geometry from neutral IR, minting fresh
/// Parasolid entity tags in a different order. The geometry survives; the tags and
/// the arena order do not. Rank-normalizing them the way [`byte_positions`]
/// normalizes container offsets would not rescue the comparison, because the
/// re-authored tags do not preserve the original relative order either — and a
/// normalization wide enough to absorb that would also absorb a writer that
/// dropped a record, which is the thing this suite must not do.
#[test]
fn an_edited_depth_survives_the_semantic_write_path() {
    let mut edited_count = 0usize;
    for (name, bytes) in harness().fixture_inputs() {
        let ran = mutation_roundtrip(
            &SldprtCodec,
            &name,
            &bytes,
            WritePath::Patched,
            |ir| {
                let depths = blind_extrude_lengths(ir);
                if depths.is_empty() {
                    return false;
                }
                for depth in depths {
                    depth.0 += MUTATION_MM;
                }
                true
            },
            |outcome| match outcome {
                MutationOutcome::Written { edited, bytes, .. } => {
                    let mut written = SldprtCodec
                        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
                        .unwrap_or_else(|error| {
                            panic!("fixture `{name}`: written container does not decode: {error}")
                        });
                    let mut expected = edited.clone();
                    let moved = depths(&mut expected);
                    let returned = depths(&mut written.ir);
                    assert_eq!(
                        returned.len(),
                        moved.len(),
                        "fixture `{name}`: the written container states {} blind depths where the edited \
                         document states {}; the edited feature was dropped or duplicated",
                        returned.len(),
                        moved.len()
                    );
                    for (index, (returned, moved)) in returned.iter().zip(&moved).enumerate() {
                        assert!(
                            (returned - moved).abs() <= 1e-9,
                            "fixture `{name}`: blind depth {index} came back as {returned} rather than \
                             {moved}; the edit was dropped"
                        );
                    }
                    assert_eq!(
                        arena_sizes(&written.ir),
                        arena_sizes(edited),
                        "fixture `{name}`: the re-authored B-rep is not the same size as the one that was \
                         written; entities were lost or invented"
                    );
                }
                MutationOutcome::Refused { error } => panic!(
                    "fixture `{name}`: the writer declined to move an extrusion depth: {error}"
                ),
            },
        );
        if ran {
            edited_count += 1;
        }
    }
    assert_eq!(
        edited_count, FIXTURES_WITH_A_BLIND_DEPTH,
        "the number of fixtures carrying a one-sided blind extrusion changed; this lane is the only \
         one that reaches `sync_neutral_features` from the corpus, so a drop here narrows it silently"
    );
}

/// Every blind depth in `ir`, by value.
fn depths(ir: &mut cadmpeg_ir::CadIr) -> Vec<f64> {
    blind_extrude_lengths(ir)
        .into_iter()
        .map(|length| length.0)
        .collect()
}

/// The size of each topological arena, so a rewrite that loses one fails.
fn arena_sizes(ir: &cadmpeg_ir::CadIr) -> [usize; 8] {
    let model = &ir.model;
    [
        model.bodies.len(),
        model.regions.len(),
        model.shells.len(),
        model.faces.len(),
        model.loops.len(),
        model.coedges.len(),
        model.edges.len(),
        model.vertices.len(),
    ]
}

/// How many committed fixtures carry a one-sided blind extrusion.
///
/// Most of this corpus covers geometry and container structure and holds no
/// feature history at all, so the lane above is narrow by construction rather
/// than by omission.
const FIXTURES_WITH_A_BLIND_DEPTH: usize = 1;
