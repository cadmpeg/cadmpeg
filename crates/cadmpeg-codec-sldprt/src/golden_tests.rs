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
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{ExtrudeExtent, FeatureDefinition, FeatureOperation, LinearTermination};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_test_support::golden::{snapshot_text, snapshots_agree, Branch, Harness};
use cadmpeg_test_support::roundtrip::{semantic_roundtrip, verbatim_replay_holds, SemanticOutcome};

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
        Branch::named("inspect", inspect_snapshot),
        Branch::named("decode", decode_snapshot),
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
                let result = cadmpeg_test_support::EditableDecodeResult::from(result);
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
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
            .unwrap();
        if decoded.source_fidelity().unrepresented().is_empty() {
            verbatim_replay_holds(&SldprtCodec, &name, &bytes);
        } else {
            use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
            let plan = SldprtCodec
                .plan(
                    EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
                    TargetRequest::Inherit,
                )
                .unwrap();
            assert!(
                matches!(plan.report().write_path(), WritePath::Synthesized { .. }),
                "fixture {name}"
            );
        }
    }
}

/// The semantic write path either reproduces a fixture's document or declares
/// the edit it cannot make.
#[test]
fn fixtures_survive_the_semantic_write_path() {
    let mut written_count = 0usize;
    let mut unresolved_selection_count = 0usize;
    for (name, bytes) in harness().fixture_inputs() {
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
            .unwrap();
        let configurations = &decoded.ir().model.configurations;
        let expected = neutral_document(&bytes);
        semantic_roundtrip(&SldprtCodec, &name, &bytes, |outcome| match outcome {
            SemanticOutcome::Written { report, bytes, .. } => {
                written_count += 1;
                assert!(
                    !matches!(report.write_path(), WritePath::VerbatimReplay { .. }),
                    "fixture {name}: semantic roundtrip did not run a writer"
                );
                if report.losses.is_empty() {
                    if let Err(mismatch) = snapshots_agree(&expected, &neutral_document(bytes)) {
                        panic!("fixture {name}: a lossless semantic write changed the document: {mismatch}");
                    }
                }
            }
            SemanticOutcome::Refused { error } => {
                // C: both frozen source partitions are held, but neither is
                // selected. A writer must not invent an active configuration.
                assert_eq!(configurations.len(), 2, "fixture {name}: {error}");
                assert!(
                    configurations
                        .iter()
                        .all(|configuration| !configuration.active),
                    "fixture {name}: {error}"
                );
                let slots = configurations
                    .iter()
                    .filter_map(|configuration| configuration.source_index)
                    .collect::<BTreeSet<_>>();
                assert_eq!(
                    slots,
                    BTreeSet::from([0, 1]),
                    "fixture {name}: exact source partition slots"
                );
                let bodies = configurations
                    .iter()
                    .flat_map(|configuration| configuration.bodies.iter().flatten())
                    .collect::<BTreeSet<_>>();
                assert_eq!(
                    bodies.len(),
                    2,
                    "fixture {name}: distinct source bodies stay held"
                );
                assert!(
                    matches!(&error, cadmpeg_core::CodecError::Malformed(message)
                    if message == "SLDPRT writing requires exactly one active configuration"),
                    "fixture {name}: {error}"
                );
                unresolved_selection_count += 1;
            }
        });
    }
    assert_eq!(
        unresolved_selection_count, 1,
        "exactly the unresolved two-partition source is refused"
    );
    assert!(
        written_count > 0,
        "no fixture reached the semantic writer, so this test exercised nothing; \
         add a fixture the writer can write, or this lane is dead"
    );
}

/// How far the mutation lane moves an extrusion depth, in millimetres.
const MUTATION_MM: f64 = 3.0;

/// One-sided blind extrusion depth in a definition.
fn blind_depth(
    definition: &FeatureDefinition,
) -> Option<&cadmpeg_ir::Dimension<cadmpeg_ir::scalar::NonZeroLength>> {
    let FeatureDefinition::Operation(FeatureOperation::Extrude {
        extent: ExtrudeExtent::OneSided { side },
        ..
    }) = definition
    else {
        return None;
    };
    match &side.termination {
        LinearTermination::Blind { length } => Some(length),
        _ => None,
    }
}

/// Canonical edits invalidate each changed feature and its dependents.
fn edit_blind_extrude_lengths(ir: &mut cadmpeg_ir::CadIr) -> usize {
    let edits = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            let dimension = blind_depth(feature.definition())?;
            let length = ir.dimension_value(dimension)?;
            Some((feature.id.clone(), length, dimension.parameter().cloned()))
        })
        .collect::<Vec<_>>();
    let configuration_edits = ir
        .model
        .configurations
        .iter()
        .enumerate()
        .flat_map(|(configuration, row)| {
            row.feature_states
                .iter()
                .filter_map(move |(feature, state)| {
                    Some((configuration, feature.clone(), state.definition()?.clone()))
                })
        })
        .filter_map(|(configuration, feature, definition)| {
            let dimension = blind_depth(&definition)?;
            let length = ir.dimension_value(dimension)?;
            let parameter = dimension.parameter().cloned();
            Some((configuration, feature, definition, length, parameter))
        })
        .collect::<Vec<_>>();
    let mut edited_parameters = BTreeSet::new();
    for (feature, length, parameter) in &edits {
        if parameter
            .as_ref()
            .is_some_and(|parameter| !edited_parameters.insert(parameter.clone()))
        {
            continue;
        }
        ir.set_dimension(
            feature,
            "/extent/side/termination/length",
            cadmpeg_ir::scalar::NonZeroLength::new(length.get() + MUTATION_MM).unwrap(),
        )
        .expect("edit the canonical blind depth");
    }
    for (configuration, feature, definition, length, parameter) in &configuration_edits {
        if parameter
            .as_ref()
            .is_some_and(|parameter| !edited_parameters.insert(parameter.clone()))
        {
            continue;
        }
        let index = ir
            .model
            .features
            .iter()
            .position(|row| row.id == *feature)
            .expect("configuration feature exists");
        let base = ir.model.features[index].definition().clone();
        ir.model.features[index]
            .evaluation
            .set_definition(definition.clone());
        let result = ir.set_dimension(
            feature,
            "/extent/side/termination/length",
            cadmpeg_ir::scalar::NonZeroLength::new(length.get() + MUTATION_MM).unwrap(),
        );
        let edited_definition = ir.model.features[index].definition().clone();
        ir.model.features[index].evaluation.set_definition(base);
        result.expect("edit the configuration's canonical blind depth");
        ir.model.configurations[*configuration]
            .feature_states
            .insert(
                feature.clone(),
                cadmpeg_ir::features::ConfigurationFeatureState::Definition {
                    definition: Box::new(edited_definition),
                },
            );
    }
    edits.len() + configuration_edits.len()
}

/// A blind-depth edit retains its solved cache and writes the native stale flag.
#[test]
fn edited_blind_depth_writes_design_and_reports_stale_brep() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
    const DEPTH_TOLERANCE_MM: f64 = 1.0e-9;
    let mut edited_count = 0usize;
    for (name, bytes) in harness().fixture_inputs() {
        let decoded = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("fixture {name}: {error}"));
        let (mut edited, _, fidelity) = decoded.into_parts();
        if edit_blind_extrude_lengths(&mut edited) == 0 {
            continue;
        }
        let plan = SldprtCodec
            .plan(
                EncodeInput::new(&edited, Some(&fidelity)),
                TargetRequest::Inherit,
            )
            .unwrap_or_else(|error| panic!("fixture {name}: {error}"));
        assert!(
            matches!(plan.report().write_path(), WritePath::Patched { .. }),
            "fixture {name}: path={:?}; losses={:?}",
            plan.report().write_path(),
            plan.report().losses,
        );
        let mut bytes = Vec::new();
        plan.write_to(&mut bytes).unwrap();
        let written = SldprtCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap();
        let synthesized = SldprtCodec
            .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
            .unwrap();
        assert!(
            matches!(
                synthesized.report().write_path(),
                WritePath::Synthesized { .. }
            ),
            "fixture {name}: source-free writer path"
        );
        let synthesized_report = synthesized.report().clone();
        let mut synthesized_bytes = Vec::new();
        synthesized.write_to(&mut synthesized_bytes).unwrap();
        let source_free = SldprtCodec
            .decode(
                &mut Cursor::new(synthesized_bytes),
                &DecodeOptions::default(),
            )
            .unwrap();
        cadmpeg_test_support::contract::assert_written_arenas_agree(
            &synthesized_report,
            written.ir(),
            source_free.ir(),
        );
        let native = crate::test_support::native::sldprt_native(written.ir());
        for feature in &written.ir().model.features {
            let Some(dimension) = blind_depth(feature.definition()) else {
                continue;
            };
            let parameter_id = dimension.parameter().expect("one canonical bound depth");
            let parameter = written
                .ir()
                .model
                .parameters
                .iter()
                .find(|parameter| parameter.id == *parameter_id)
                .unwrap();
            assert_eq!(
                parameter.owner.as_ref(),
                Some(&feature.id),
                "fixture {name}: bound owner"
            );
            let cache = parameter
                .value
                .as_ref()
                .expect("evaluated canonical length");
            assert_eq!(
                parameter.expression.literal_value_as(cache).as_ref(),
                Some(cache),
                "fixture {name}: exact expression/cache agreement"
            );
            let native_feature = native
                .feature_histories
                .iter()
                .flat_map(|history| &history.features)
                .find(|native_feature| {
                    Some(native_feature.id.as_str()) == feature.native_ref.as_deref()
                })
                .expect("native Keywords counterpart");
            let native_text = &native_feature.parameters[parameter.name.as_str()];
            assert_eq!(
                cadmpeg_ir::features::Expression::text(native_text.as_str())
                    .literal_value_as(cache)
                    .as_ref(),
                Some(cache),
                "fixture {name}: native Keywords depth regenerated from canonical parameter"
            );
        }
        let mut written = written.ir().clone();
        assert!(
            !written.model.features.is_empty()
                && written
                    .model
                    .features
                    .iter()
                    .all(|feature| feature.needs_recompute),
            "fixture {name}: SolidWorks writes swConfigurationNeedsUpdate=YES for stale evaluation"
        );
        let expected = depths(&mut edited);
        let actual = depths(&mut written);
        assert_eq!(
            actual.len(),
            expected.len(),
            "fixture {name}: an edited feature was dropped or duplicated"
        );
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(
                (actual - expected).abs() <= DEPTH_TOLERANCE_MM,
                "fixture {name}: edited depth was discarded"
            );
        }
        assert_eq!(
            written.model.faces.len(),
            edited.model.faces.len(),
            "fixture {name}: stale solved carrier changed"
        );
        edited_count += 1;
    }
    assert_eq!(edited_count, FIXTURES_WITH_A_BLIND_DEPTH);
}

/// Every blind depth in `ir`, by value.
fn depths(ir: &mut cadmpeg_ir::CadIr) -> Vec<f64> {
    ir.model
        .features
        .iter()
        .map(cadmpeg_ir::Feature::definition)
        .chain(
            ir.model
                .configurations
                .iter()
                .flat_map(|configuration| configuration.feature_states.values())
                .filter_map(cadmpeg_ir::features::ConfigurationFeatureState::definition),
        )
        .filter_map(blind_depth)
        .map(|dimension| ir.dimension_value(dimension).expect("resolved depth").get())
        .collect()
}

/// How many committed fixtures carry a one-sided blind extrusion.
const FIXTURES_WITH_A_BLIND_DEPTH: usize = 1;

/// A geometry-depth decode agrees with the full decode outside the design
/// arenas. This codec has no geometry-depth cut, so every input delivers full.
#[test]
fn geometry_depth_agrees_with_full_depth() {
    let summary = cadmpeg_test_support::depth::assert_geometry_agrees_with_full_with(
        &SldprtCodec,
        &harness().fixture_inputs(),
        |decoded| {
            let native = decoded
                .ir()
                .native
                .namespace("sldprt")
                .map(crate::native::SldprtNative::load)
                .transpose()
                .expect("typed native records");
            let expected = crate::loss::unrepresented(&decoded.report().losses, native.as_ref());
            assert!(
                expected
                    .iter()
                    .all(|class| decoded.source_fidelity().unrepresented().contains(class)),
                "each absent source class must be declared"
            );
        },
    );
    assert_eq!(summary.cut, 0, "{summary:?}");
}
