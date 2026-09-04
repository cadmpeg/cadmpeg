// SPDX-License-Identifier: Apache-2.0
//! Golden snapshot harness for `inspect` and `decode` over the committed
//! fixtures.
//!
//! `tests/golden/fixtures/*.sldprt` are the frozen inputs.
//! Fixtures stay frozen; `UPDATE_GOLDEN=1` rewrites goldens only.
//! `inspect` pins the container summary; `decode` pins the IR, losses, and
//! source fidelity. Shared harness: [`cadmpeg_test_support::golden`].

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{ExtrudeExtent, FeatureDefinition, Length, LinearTermination};
use cadmpeg_ir::WritePath;
use cadmpeg_test_support::golden::{
    elide_local_digests, snapshot_text, snapshots_agree, Branch, Harness,
};
use cadmpeg_test_support::roundtrip::{
    mutation_roundtrip, semantic_roundtrip, verbatim_replay_holds, MutationOutcome, SemanticOutcome,
};

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
            Ok(result) => {
                let mut result = cadmpeg_test_support::EditableDecodeResult::from(result);
                if let Some(source) = result.ir_mut().source.as_mut() {
                    // The `native` lane digests cover retained source bytes and
                    // stay pinned; a `_local_sha256` digest covers decoded
                    // content, so a platform's libm moves it.
                    elide_local_digests(&mut source.attributes);
                }
                serde_json::json!({
                    "ir": serde_json::to_value(result.ir()).expect("serialize ir"),
                    "report": serde_json::to_value(result.report()).expect("serialize report"),
                    "source_fidelity": serde_json::to_value(result.source_fidelity())
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
/// `brep` and `appearance` are absent: their heads are Parasolid tags inside
/// replayed payloads and must compare exactly.
const SECTION_SCOPED_IDENTIFIERS: [&str; 6] = [
    "sldprt:displaylist:",
    "sldprt:feature-input:",
    "sldprt:file:",
    "sldprt:history:",
    "sldprt:metadata:",
    "sldprt:model:",
];

/// Every container byte position one identifier carries, as
/// `(family, position)` pairs sharing a rank space. Only the section-ordinal
/// key head is normalized; remaining components compare exactly.
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
    let (head, _) = split_key(key);
    let Ok(section) = head.parse::<u64>() else {
        return Vec::new();
    };
    vec![(String::from("section"), section)]
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
/// Identifiers embed container byte offsets, so a repack renames entities
/// without changing relative order; ranks capture that order. The snapshot
/// covers the model, units, and tolerances only — not `ir.native` or
/// `ir.source.attributes`, which describe the rebuilt container.
fn neutral_document(bytes: &[u8]) -> String {
    let value =
        match SldprtCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default()) {
            Ok(result) => normalized_document(result.ir()),
            Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
        };
    snapshot_text(&value)
}

/// The model, units, and tolerances with container byte positions rank-normalized.
fn normalized_document(ir: &cadmpeg_ir::CadIr) -> serde_json::Value {
    let mut document = serde_json::json!({
        "model": serde_json::to_value(&ir.model).expect("serialize model"),
        "units": { "length": "millimeter" },
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
#[test]
fn fixtures_replay_verbatim() {
    for (name, bytes) in harness().fixture_inputs() {
        verbatim_replay_holds(&SldprtCodec, &name, &bytes);
    }
}

/// The semantic write path either reproduces a fixture's document or declares
/// the edit it cannot make.
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
const MUTATION_MM: f64 = 3.0;

/// Every statement of a one-sided blind extrusion depth in `ir`.
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
            LinearTermination::Blind { length } => Some(length),
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
                    let written = SldprtCodec
                        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
                        .unwrap_or_else(|error| {
                            panic!("fixture `{name}`: written container does not decode: {error}")
                        });
                    let mut written = cadmpeg_test_support::EditableDecodeResult::from(written);
                    let mut expected = edited.clone();
                    let moved = depths(&mut expected);
                    let returned = depths(&mut written.ir_mut());
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
                            (returned - moved).abs() <= 1.0e-9,
                            "fixture `{name}`: blind depth {index} came back as {returned} rather than \
                             {moved}; the edit was dropped"
                        );
                    }
                    assert_eq!(
                        arena_sizes(written.ir()),
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
const FIXTURES_WITH_A_BLIND_DEPTH: usize = 1;
