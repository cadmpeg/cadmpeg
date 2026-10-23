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
use cadmpeg_ir::features::{ExtrudeExtent, FeatureDefinition, FeatureOperation, LinearTermination};
use cadmpeg_ir::report::export::WritePath;
use cadmpeg_test_support::golden::{
    elide_local_digests, snapshot_text, snapshots_agree, Branch, Harness,
};
use cadmpeg_test_support::roundtrip::{
    mutation_roundtrip, semantic_roundtrip, verbatim_replay_holds, ExpectedWritePath,
    MutationOutcome, SemanticOutcome,
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

/// Compare neutral semantics after mapping source section positions through
/// their unique admitted member names. Native and source metadata are excluded.
fn neutral_document(bytes: &[u8]) -> String {
    let value =
        match SldprtCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default()) {
            Ok(result) => {
                let scan = crate::test_support::container::scan(bytes);
                let sections =
                    section_identities(&scan).expect("unique admitted source member identities");
                normalized_document(result.ir(), &sections)
            }
            Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
        };
    snapshot_text(&value)
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
        if frozen_invalid_cache_fixture(&name, &bytes) {
            continue;
        }
        verbatim_replay_holds(&SldprtCodec, &name, &bytes);
    }
}

/// The semantic write path either reproduces a fixture's document or declares
/// the edit it cannot make.
#[test]
fn fixtures_survive_the_semantic_write_path() {
    let mut written_count = 0usize;
    for (name, bytes) in harness().fixture_inputs() {
        if frozen_invalid_cache_fixture(&name, &bytes) {
            continue;
        }
        if name == "body_history" {
            assert_legacy_history_preconditions(&bytes);
            semantic_roundtrip(&SldprtCodec, &name, &bytes, |outcome| {
                match outcome {
                SemanticOutcome::Refused { error } => assert!(
                    matches!(error, CodecError::Malformed(detail) if detail == "SLDPRT writing requires exactly one active configuration"),
                    "legacy history must refuse exactly the unresolved active-configuration contract"
                ),
                SemanticOutcome::Written { .. } => panic!("legacy history lacks a supported active configuration"),
            }
            });
            continue;
        }
        let expected = neutral_document(&bytes);
        semantic_roundtrip(&SldprtCodec, &name, &bytes, |outcome| match outcome {
            SemanticOutcome::Written { report, bytes, .. } => {
                written_count += 1;
                assert!(
                    matches!(report.write_path(), WritePath::Patched { .. }),
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
fn visit_blind_extrude_lengths(
    ir: &mut cadmpeg_ir::CadIr,
    mut visit: impl FnMut(&mut cadmpeg_ir::scalar::NonZeroLength),
) -> usize {
    fn depth(definition: &mut FeatureDefinition) -> Option<&mut cadmpeg_ir::scalar::NonZeroLength> {
        let FeatureDefinition::Operation(FeatureOperation::Extrude {
            extent: ExtrudeExtent::OneSided { side },
            ..
        }) = definition
        else {
            return None;
        };
        match &mut side.termination {
            LinearTermination::Blind { length } => Some(length),
            _ => None,
        }
    }
    let mut count = 0;
    for feature in &mut ir.model.features {
        feature.evaluation.edit(|definition, _| {
            if let Some(length) = depth(definition) {
                count += 1;
                visit(length);
            }
        });
    }
    for state in ir
        .model
        .configurations
        .iter_mut()
        .flat_map(|configuration| configuration.feature_states.values_mut())
    {
        if let Some(length) = depth(&mut state.definition) {
            count += 1;
            visit(length);
        }
    }
    count
}

/// A blind-depth edit without a regenerated B-rep is refused.
#[test]
fn edited_blind_depth_without_regenerated_brep_is_refused() {
    let mut edited_count = 0usize;
    for (name, bytes) in harness().fixture_inputs() {
        if frozen_invalid_cache_fixture(&name, &bytes) {
            continue;
        }
        if name == "body_history" {
            assert_legacy_history_preconditions(&bytes);
            continue;
        }
        let ran = mutation_roundtrip(
            &SldprtCodec,
            &name,
            &bytes,
            ExpectedWritePath::Patched,
            |ir| {
                visit_blind_extrude_lengths(ir, |depth| {
                    *depth =
                        cadmpeg_ir::scalar::NonZeroLength::new(depth.get() + MUTATION_MM).unwrap();
                }) != 0
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
                    panic!("fixture `{name}`: a blind-depth edit wrote an unverified B-rep");
                }
                MutationOutcome::Refused { error } => assert!(
                    matches!(error, CodecError::NotImplemented(detail) if detail.starts_with("SLDPRT writer cannot regenerate B-rep after")),
                    "fixture `{name}`: {error}"
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
    let mut values = Vec::new();
    visit_blind_extrude_lengths(ir, |length| values.push(length.get()));
    values
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

/// The old history input still carries a depth; only the authored replacement
/// has the active configuration and B-rep required to exercise the edit guard.
const LEGACY_FIXTURES_WITH_A_BLIND_DEPTH: usize = 1;

/// A geometry-depth decode agrees with the full decode outside the design
/// arenas. This codec has no geometry-depth cut, so every input delivers full.
#[test]
fn geometry_depth_agrees_with_full_depth() {
    let summary = cadmpeg_test_support::depth::assert_geometry_agrees_with_full(
        &SldprtCodec,
        &harness().fixture_inputs(),
    );
    assert_eq!(summary.cut, 0, "{summary:?}");
}

/// One frozen historical container intentionally has a cache header at the
/// preceding block's end, instead of at a positive anchored-grid displacement.
/// This proves the malformed layout independently of the decoder's error.
fn frozen_invalid_cache_fixture(name: &str, bytes: &[u8]) -> bool {
    if name != "container_metadata" {
        return false;
    }
    let u32_at = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("frozen u32"));
    assert_eq!(bytes.len(), 264);
    assert_eq!(&bytes[54..60], &crate::container::MARKER);
    assert_eq!(u32_at(54 + 6), 0x20);
    assert_eq!(u32_at(54 + 14), 45);
    assert_eq!(u32_at(54 + 22), 27);
    let preceding_block_end = 54 + 26 + 27 + 45;
    assert_eq!(preceding_block_end, 152);
    assert_eq!(&bytes[152..158], &crate::container::MARKER);
    assert_eq!(u32_at(152 + 6), 0);
    assert_eq!(u32_at(152 + 10), 180);
    assert_eq!(u32_at(152 + 14), 45);
    assert_eq!(u32_at(152 + 18), 90);
    // L/stride must be positive for every admitted stride; the actual
    // displacement here is zero, independent of the chosen stride.
    assert_eq!(152 - preceding_block_end, 0);
    let error = SldprtCodec
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
        .expect_err("zero-displacement cache grid must be malformed");
    assert!(
        matches!(error, cadmpeg_ir::DecodeFailure::Codec(CodecError::Malformed(detail))
        if detail == "native ZIP directory or anchored cache-cell geometry is inconsistent")
    );
    true
}

/// The original history bytes lack both an active configuration source index
/// and a complete typed BODY ownership class. Keep the bytes and their raw
/// replay coverage; do not turn their unsupported semantic write into a pass.
fn assert_legacy_history_preconditions(bytes: &[u8]) {
    let scan = crate::test_support::container::scan(bytes);
    assert_eq!(crate::container::active_configuration_index(&scan), None);
    let keywords = scan
        .blocks
        .iter()
        .find(|block| block.section.name() == Some("Contents/Keywords"))
        .expect("frozen Keywords block");
    assert_eq!(keywords.payload.as_slice(), br#"<Keywords Name="Bracket"><Configuration Name="Default" Material="Steel" DisplayState="Shaded"/><Extrusion Name="Boss" Type="BossExtrude" id="7" Scope="Body1"><Dimension Name="Depth">12.5mm</Dimension><EquationDrivenCurve Name="Profile" id="8"/></Extrusion></Keywords>"#);
    let partition = scan
        .blocks
        .iter()
        .find(|block| block.section.name() == Some("Contents/Config-0-Partition"))
        .expect("frozen partition block");
    assert_eq!(partition.payload.len(), 544);
    for class in [0x0c, 0x0d, 0x13] {
        assert!(!partition
            .payload
            .windows(3)
            .any(|bytes| bytes == [0, class, 0xff]));
    }
    // The three legacy VERTEX records are only 26 bytes including their tag.
    // The would-be trailing owner is the next VERTEX/POINT class tag, not a
    // verified entity reference. These offsets were checked from retained raw
    // partition bytes independently of topology projection.
    for at in [0x15a, 0x174, 0x18e] {
        assert_eq!(&partition.payload[at..at + 2], &[0, 0x12]);
    }
    assert_eq!(&partition.payload[0x1a8..0x1aa], &[0, 0x1d]);
    let result = SldprtCodec
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
        .expect("historical history fixture retains its source");
    assert!(!result.ir().model.configurations.is_empty());
    assert_eq!(
        result
            .ir()
            .model
            .configurations
            .iter()
            .filter(|c| c.active)
            .count(),
        0
    );
    assert_eq!(arena_sizes(result.ir()), [0; 8]);
    assert!(result.ir().source.is_some());
    let mut ir = result.ir().clone();
    assert!(
        !depths(&mut ir).is_empty(),
        "legacy blind-depth coverage remains explicit"
    );
    verbatim_replay_holds(&SldprtCodec, "body_history", bytes);
}

/// Independently authored, complete sources retain positive geometry coverage
/// as the older incomplete ownership examples correctly abstain.
#[test]
fn complete_authored_golden_inputs_have_typed_geometry_and_active_configuration() {
    use cadmpeg_ir::geometry::{
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };
    let fixtures = harness().fixture_inputs();
    let cases = [
        ("authored_complete_planar", [1, 1, 1, 1, 1, 3, 3, 3]),
        ("authored_complete_history", [1, 1, 1, 1, 1, 3, 3, 3]),
        ("authored_complete_cylinder", [1, 1, 1, 1, 1, 4, 3, 2]),
        ("authored_complete_sphere", [1, 1, 1, 1, 1, 4, 4, 3]),
        ("authored_complete_nurbs", [1, 1, 1, 1, 1, 3, 3, 3]),
    ];
    for (name, expected) in cases {
        let (_, bytes) = fixtures
            .iter()
            .find(|(fixture, _)| fixture == name)
            .expect("emit and commit the complete authored fixture");
        let result = SldprtCodec
            .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
            .expect("complete authored fixture decodes");
        let ir = result.ir();
        assert_eq!(
            arena_sizes(ir),
            expected,
            "{name}: exact topological arenas"
        );
        assert_eq!(
            ir.model.configurations.iter().filter(|c| c.active).count(),
            1,
            "{name}"
        );
        assert_eq!(ir.model.surfaces.len(), 1, "{name}: one face carrier");
        let expected_curves = match name {
            "authored_complete_planar" | "authored_complete_history" => 0,
            "authored_complete_cylinder" => 3,
            "authored_complete_sphere" => 4,
            "authored_complete_nurbs" => 1,
            _ => unreachable!(),
        };
        assert_eq!(
            ir.model.curves.len(),
            expected_curves,
            "{name}: exact curve carriers"
        );
        let surface = &ir.model.surfaces[0].geometry;
        match name {
            "authored_complete_planar" | "authored_complete_history" => assert!(matches!(
                surface,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
            )),
            "authored_complete_cylinder" => assert!(matches!(
                surface,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_))
            )),
            "authored_complete_sphere" => assert!(matches!(
                surface,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_))
            )),
            "authored_complete_nurbs" => {
                let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) = surface else {
                    panic!("complete NURBS fixture lost its surface carrier");
                };
                assert_eq!((nurbs.u_degree(), nurbs.v_degree()), (1, 1));
                assert_eq!((nurbs.u_count(), nurbs.v_count()), (2, 2));
                assert_eq!(nurbs.poles().len(), 4);
                assert_eq!(
                    ir.model
                        .curves
                        .iter()
                        .filter(|curve| matches!(
                            curve.geometry,
                            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_))
                        ))
                        .count(),
                    1
                );
            }
            _ => unreachable!(),
        }
        if name == "authored_complete_history" {
            assert_eq!(ir.model.features.len(), 2);
            let mut ir = ir.clone();
            let lengths = depths(&mut ir);
            assert!(!lengths.is_empty());
            assert!(lengths.iter().all(|length| *length == 12.5));
        }
    }
    assert_eq!(
        fixtures
            .iter()
            .filter(|(name, _)| name == "body_history")
            .count(),
        LEGACY_FIXTURES_WITH_A_BLIND_DEPTH
    );
}

mod authored_coverage;

/// Match physical section ordinals only through unique admitted source names.
fn section_identities(
    scan: &crate::container::ContainerScan<'_>,
) -> Result<BTreeMap<u64, String>, &'static str> {
    let mut names = BTreeSet::new();
    let mut identities = BTreeMap::new();
    for section in scan.sections() {
        let name = section
            .name()
            .ok_or("unnamed source section cannot be normalized")?;
        if !names.insert(name) {
            return Err("duplicate source section name cannot be normalized");
        }
        let position = u64::try_from(section.ordinal())
            .map_err(|_| "source section ordinal does not fit u64")?;
        let mut identity = String::from("<section=");
        for byte in name.as_bytes() {
            use std::fmt::Write as _;
            write!(&mut identity, "{byte:02x}").expect("write canonical section identity");
        }
        identity.push('>');
        if identities.insert(position, identity).is_some() {
            return Err("duplicate source section ordinal cannot be normalized");
        }
    }
    Ok(identities)
}

fn normalize_identifier(id: &str, sections: &BTreeMap<u64, String>) -> Option<String> {
    if !SECTION_SCOPED_IDENTIFIERS
        .iter()
        .any(|scope| id.starts_with(scope))
    {
        return None;
    }
    let key = identifier_key(id)?;
    let (head, rest) = split_key(key);
    let position = head.parse::<u64>().ok()?;
    let identity = sections.get(&position)?;
    let (prefix, _) = id.rsplit_once('#')?;
    Some(match rest {
        Some(rest) => format!("{prefix}#{identity}:{rest}"),
        None => format!("{prefix}#{identity}"),
    })
}

/// Arena storage order is not a semantic nested-list order.
fn canonicalize_model_arena_order(document: &mut serde_json::Value) {
    let Some(model) = document
        .get_mut("model")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    for arena in model.values_mut() {
        let Some(rows) = arena.as_array_mut() else {
            continue;
        };
        let ids = rows
            .iter()
            .map(|row| row.get("id").and_then(serde_json::Value::as_str))
            .collect::<Option<BTreeSet<_>>>();
        if ids.is_some_and(|ids| ids.len() == rows.len()) {
            rows.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
        }
    }
}

fn normalized_document(
    ir: &cadmpeg_ir::CadIr,
    sections: &BTreeMap<u64, String>,
) -> serde_json::Value {
    let ctx = cadmpeg_test_support::service_decode_context();
    let original_model = serde_json::to_value(&ir.model).expect("serialize original model");
    let mut opaque_references = BTreeMap::new();
    let mut declared_replacements = BTreeMap::new();
    let mut model = cadmpeg_ir::document::Model::default();
    model
        .extend_rewritten(
            ir.model.clone(),
            &mut DeclaredSectionIdentityRewrite {
                ctx: &ctx,
                sections,
                opaque_references: &mut opaque_references,
                declared_replacements: &mut declared_replacements,
            },
        )
        .expect("rewrite only schema-declared source identities");
    let mut normalized_model = serde_json::to_value(&model).expect("serialize model");
    restore_typed_opaque_references(
        &original_model,
        &mut normalized_model,
        &opaque_references,
        &declared_replacements,
    );
    let mut document = serde_json::json!({
        "model": normalized_model,
        "units": { "length": "millimeter" },
        "tolerances": serde_json::to_value(ir.tolerances).expect("serialize tolerances"),
    });
    canonicalize_model_arena_order(&mut document);
    document
}

#[test]
fn normalization_replaces_only_container_positions() {
    let sections = BTreeMap::from([
        (247_u64, "<keywords>".to_owned()),
        (400, "<resolved>".to_owned()),
    ]);
    assert_ne!(
        normalize_identifier("sldprt:model:feature#247:0", &sections),
        normalize_identifier("sldprt:model:feature#400:0", &sections)
    );
    assert_ne!(
        normalize_identifier("sldprt:metadata:part_record#247:0", &sections),
        normalize_identifier("sldprt:metadata:part_record#247:269", &sections)
    );
    assert_eq!(
        normalize_identifier("sldprt:model:sketch-entity#247:0:0:1", &sections).as_deref(),
        Some("sldprt:model:sketch-entity#<keywords>:0:0:1")
    );
    assert_ne!(
        normalize_identifier("sldprt:model:sketch-entity#247:0:0:1", &sections),
        normalize_identifier("sldprt:model:sketch-entity#247:0:0:2", &sections)
    );
    assert_eq!(
        normalize_identifier("sldprt:brep:face#247", &sections),
        None
    );
    assert_eq!(
        normalize_identifier("sldprt:appearance:entity53#247", &sections),
        None
    );
    assert_eq!(
        normalize_identifier("sldprt:feature-input:class#247:106", &sections).as_deref(),
        Some("sldprt:feature-input:class#<keywords>:106")
    );
    assert_eq!(
        normalize_identifier("sldprt:model:feature#999:0", &sections),
        None
    );
}

#[test]
fn normalized_comparison_follows_unique_members_when_their_physical_order_changes() {
    use crate::test_support::container::{make_block, native_envelope_fixture, outer_header, scan};
    let fixtures = harness().fixture_inputs();
    let (_, bytes) = fixtures
        .iter()
        .find(|(name, _)| name == "authored_nested_arc")
        .expect("authored complete arc fixture");
    let indexed = scan(bytes);
    let mut blocks = indexed.blocks.iter().collect::<Vec<_>>();
    let before = blocks
        .iter()
        .map(|block| block.section.name())
        .collect::<Vec<_>>();
    blocks.reverse();
    assert_ne!(
        before,
        blocks
            .iter()
            .map(|block| block.section.name())
            .collect::<Vec<_>>()
    );
    let mut source = outer_header();
    for block in blocks {
        source.extend(make_block(
            block.type_id,
            block.section.name().expect("named fixture member"),
            &block.payload,
        ));
    }
    let reordered = native_envelope_fixture(&source);
    assert_ne!(bytes, &reordered);
    assert_eq!(neutral_document(bytes), neutral_document(&reordered));
}

#[test]
fn normalized_comparison_preserves_references_depth_values_and_nested_order() {
    let fixtures = harness().fixture_inputs();
    let (_, bytes) = fixtures
        .iter()
        .find(|(name, _)| name == "authored_complete_history")
        .expect("authored complete history fixture");
    let indexed = crate::test_support::container::scan(bytes);
    let sections = section_identities(&indexed).expect("unique authored member names");
    let result = SldprtCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("complete history decode");
    let original = normalized_document(result.ir(), &sections);
    let mut changed = result.ir().clone();
    changed.model.coedges[0].owner_loop =
        cadmpeg_ir::ids::LoopId::mint("sldprt:brep:loop#999").expect("authored distinct reference");
    assert_ne!(original, normalized_document(&changed, &sections));
    let mut changed = result.ir().clone();
    assert!(
        visit_blind_extrude_lengths(&mut changed, |length| {
            *length = cadmpeg_ir::scalar::NonZeroLength::new(length.get() + 1.0)
                .expect("positive changed depth");
        }) > 0
    );
    assert_ne!(original, normalized_document(&changed, &sections));
    let mut ordered = serde_json::json!({"model": {"entities": [
        {"id": "b", "members": ["a", "b"]}, {"id": "a", "members": []}
    ]}});
    canonicalize_model_arena_order(&mut ordered);
    let mut changed = ordered.clone();
    changed["model"]["entities"][1]["members"]
        .as_array_mut()
        .expect("semantic list")
        .reverse();
    canonicalize_model_arena_order(&mut changed);
    assert_ne!(ordered, changed);
}

#[test]
fn duplicate_source_member_names_are_not_conflated() {
    use crate::test_support::container::{make_block, outer_header, scan};
    let mut source = outer_header();
    source.extend(make_block(0x40, "Contents/Duplicate", b"first"));
    source.extend(make_block(0x40, "Contents/Duplicate", b"second"));
    let indexed = scan(&source);
    assert_eq!(indexed.sections().count(), 2);
    assert_eq!(
        section_identities(&indexed),
        Err("duplicate source section name cannot be normalized")
    );
}

struct DeclaredSectionIdentityRewrite<'a, 'ctx> {
    ctx: &'a cadmpeg_core::decode::DecodeContext<'ctx>,
    sections: &'a BTreeMap<u64, String>,
    opaque_references: &'a mut BTreeMap<String, String>,
    declared_replacements: &'a mut BTreeMap<String, String>,
}

fn hex_reference(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        write!(encoded, "{byte:02x}").unwrap();
    }
    encoded
}

impl cadmpeg_ir::document::EntityRewrite for DeclaredSectionIdentityRewrite<'_, '_> {
    type Error = CodecError;

    fn rewrite<T: cadmpeg_ir::schema::rewrite::typed::RewriteIdentities>(
        &mut self,
        entity: T,
    ) -> Result<T, Self::Error> {
        cadmpeg_ir::schema::rewrite::identities(self.ctx, entity, |source| {
            let target = if cadmpeg_ir::ids::Identity::new(source).is_ok() {
                normalize_identifier(source, self.sections).unwrap_or_else(|| source.to_owned())
            } else {
                // Full-fidelity reference fields can carry codec-qualified native
                // wire references which are not neutral Identity values. Admit a
                // temporary injective identity only at this typed owner callback.
                let key = hex_reference(source);
                let token = format!("sldprt:golden-opaque:reference#{key}");
                self.opaque_references
                    .insert(token.clone(), source.to_owned());
                token
            };
            self.declared_replacements
                .insert(source.to_owned(), target.clone());
            Ok(target)
        })
    }
}

#[test]
fn section_normalization_preserves_identity_shaped_labels_and_unowned_properties() {
    let fixtures = harness().fixture_inputs();
    let (_, bytes) = fixtures
        .iter()
        .find(|(name, _)| name == "authored_complete_history")
        .expect("authored complete history fixture");
    let indexed = crate::test_support::container::scan(bytes);
    let sections = section_identities(&indexed).expect("unique authored source members");
    let result = SldprtCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("complete history source decode");
    let mut labeled = result.ir().clone();
    let source_id = labeled.model.features[0].id.as_str().to_owned();
    let rewritten_id = normalize_identifier(&source_id, &sections)
        .expect("physical source section in declared ID");
    assert_ne!(source_id, rewritten_id);
    labeled.model.features[0].name = Some(source_id.clone());
    let normalized = normalized_document(&labeled, &sections);
    let feature = normalized["model"]["features"]
        .as_array()
        .expect("feature arena")
        .iter()
        .find(|feature| feature["id"] == rewritten_id)
        .expect("declared ID normalized");
    assert_eq!(feature["name"], source_id);
    let mut changed = labeled.clone();
    changed.model.features[0].name = Some(rewritten_id.clone());
    assert_ne!(normalized, normalized_document(&changed, &sections));

    let key = cadmpeg_core::nonblank_literal!("unowned_identity_shaped_text");
    labeled.model.features[0]
        .source_properties
        .insert(key.clone(), source_id.clone());
    let normalized = normalized_document(&labeled, &sections);
    let feature = normalized["model"]["features"]
        .as_array()
        .expect("feature arena")
        .iter()
        .find(|feature| feature["id"] == rewritten_id)
        .expect("declared ID normalized");
    assert_eq!(feature["source_properties"][key.as_str()], source_id);
    let mut changed = labeled.clone();
    changed.model.features[0]
        .source_properties
        .insert(key, rewritten_id);
    assert_ne!(normalized, normalized_document(&changed, &sections));
}

/// Restore only a leaf changed by the preceding typed owner walk. This paired
/// structural check never classifies an unchanged ordinary string as a reference.
fn restore_typed_opaque_references(
    original: &serde_json::Value,
    rewritten: &mut serde_json::Value,
    opaque: &BTreeMap<String, String>,
    declared: &BTreeMap<String, String>,
) {
    match (original, rewritten) {
        (serde_json::Value::String(source), serde_json::Value::String(target)) => {
            if source != target && opaque.get(target) == Some(source) {
                *target = source.clone();
            }
        }
        (serde_json::Value::Array(before), serde_json::Value::Array(after)) => {
            assert_eq!(
                before.len(),
                after.len(),
                "typed rewrite preserves nested sequence length"
            );
            for (source, target) in before.iter().zip(after) {
                restore_typed_opaque_references(source, target, opaque, declared);
            }
        }
        (serde_json::Value::Object(before), serde_json::Value::Object(after)) => {
            assert_eq!(
                before.len(),
                after.len(),
                "typed rewrite preserves field roster"
            );
            for (key, source) in before {
                // Typed identity-keyed maps can rename a key. Ordinary map keys
                // are retained, even when their spelling also names an identity.
                let target_key = if after.contains_key(key) {
                    key
                } else {
                    declared
                        .get(key)
                        .expect("typed rewrite owns a changed map key")
                };
                let target = after.get_mut(target_key).expect("typed rewritten field");
                restore_typed_opaque_references(source, target, opaque, declared);
            }
        }
        _ => {}
    }
}

#[test]
fn section_normalization_preserves_qualified_native_reference_grammar() {
    let fixtures = harness().fixture_inputs();
    let (_, bytes) = fixtures
        .iter()
        .find(|(name, _)| name == "authored_complete_history")
        .expect("authored complete history fixture");
    let indexed = crate::test_support::container::scan(bytes);
    let sections = section_identities(&indexed).expect("unique source members");
    let result = SldprtCodec
        .decode(&mut Cursor::new(bytes.clone()), &DecodeOptions::default())
        .expect("complete history source decode");
    let mut source = result.ir().clone();
    let reference = "0:sldprt:brep:edge#40";
    source.model.features[0].native_ref = Some(reference.to_owned());
    source.model.features[0].name = Some(reference.to_owned());
    let id = normalize_identifier(source.model.features[0].id.as_str(), &sections)
        .expect("source-backed feature identity");
    let normalized = normalized_document(&source, &sections);
    let feature = normalized["model"]["features"]
        .as_array()
        .expect("feature arena")
        .iter()
        .find(|feature| feature["id"] == id)
        .expect("normalized feature");
    assert_eq!(feature["native_ref"], reference);
    assert_eq!(feature["name"], reference);
    for changed_reference in ["1:sldprt:brep:edge#40", "0:sldprt:brep:edge#41"] {
        let mut changed = source.clone();
        changed.model.features[0].native_ref = Some(changed_reference.to_owned());
        assert_ne!(normalized, normalized_document(&changed, &sections));
    }
    let mut changed = source.clone();
    changed.model.features[0].name = Some("1:sldprt:brep:edge#40".to_owned());
    assert_ne!(normalized, normalized_document(&changed, &sections));
    let token = format!(
        "sldprt:golden-opaque:reference#{}",
        hex_reference(reference)
    );
    source.model.features[0].name = Some(token.clone());
    let normalized = normalized_document(&source, &sections);
    let feature = normalized["model"]["features"]
        .as_array()
        .expect("feature arena")
        .iter()
        .find(|feature| feature["id"] == id)
        .expect("normalized feature");
    assert_eq!(
        feature["name"], token,
        "ordinary text equal to a temporary token stays exact"
    );
    assert_eq!(feature["native_ref"], reference);
}
