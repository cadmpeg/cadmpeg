// SPDX-License-Identifier: Apache-2.0
//! Golden snapshot harness for `inspect`, `decode`, and `encode` over the
//! committed fixtures.
//!
//! `tests/fixtures/*.p21` are the frozen inputs.
//! Fixtures stay frozen; `UPDATE_GOLDEN=1` rewrites goldens only.
//! `inspect` pins the container summary; `decode` pins the IR, losses, and
//! source fidelity; `encode` pins the written schema, the `ExportReport`
//! target, and deliberate refusals. Shared harness:
//! [`cadmpeg_test_support::golden`].
//!
//! `inspect` and `decode` run over every fixture. `encode` runs over
//! [`ENCODE_FIXTURES`] only: a written STEP file is far larger than the report
//! that describes it, and four sources already span every target resolution
//! the writer has.

use std::io::Cursor;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::golden::{snapshot_text, Branch, Harness};

use super::{StepCodec, StepSchema};

/// Extension of the committed fixture inputs.
const FIXTURE_EXTENSION: &str = "p21";

/// Crate-relative regeneration hint used in every failure message.
const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-step golden";

fn harness() -> Harness {
    Harness::new(env!("CARGO_MANIFEST_DIR"), FIXTURE_EXTENSION, REGENERATE)
        .with_fixture_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
}

/// The branches every fixture passes through, in golden-directory order.
fn branches() -> [Branch; 2] {
    [
        Branch::named("inspect", inspect_snapshot),
        Branch::named("decode", decode_snapshot),
    ]
}

/// The single encode branch, run over [`ENCODE_FIXTURES`].
fn encode_branch() -> [Branch; 1] {
    [Branch::named("encode", encode_snapshot)]
}

/// Fixture stems the encode branch covers.
///
/// One per way `plan` can resolve a target from a source. `ap203_sheet`
/// declares `CONFIG_CONTROL_DESIGN`, so `Inherit` reproduces a non-default row;
/// `ap214_sheet` declares `AUTOMOTIVE_DESIGN`, so `Inherit` reproduces the
/// catalog default; `ap242_minimal` carries an edition in its schema object
/// identifier; and `ap242_geometry` declares `AP242_..._MIM_LF` with no
/// edition, which names no catalog row and must refuse rather than guess one.
const ENCODE_FIXTURES: [&str; 4] = [
    "ap203_sheet",
    "ap214_sheet",
    "ap242_minimal",
    "ap242_geometry",
];

/// The fixtures in [`ENCODE_FIXTURES`], as harness inputs.
fn encode_inputs() -> Vec<(String, Vec<u8>)> {
    let all = harness().fixture_inputs();
    let selected: Vec<(String, Vec<u8>)> = all
        .into_iter()
        .filter(|(name, _)| ENCODE_FIXTURES.contains(&name.as_str()))
        .collect();
    assert_eq!(
        selected.len(),
        ENCODE_FIXTURES.len(),
        "ENCODE_FIXTURES names a stem with no `.{FIXTURE_EXTENSION}` fixture"
    );
    selected
}

/// Serializes one container summary. An inspect error is frozen too: refusing a
/// container is contract-relevant behavior, so this never panics on codec
/// output.
fn inspect_snapshot(bytes: &[u8]) -> String {
    let value = match StepCodec::default()
        .inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default())
    {
        Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect summary"),
        Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
    };
    snapshot_text(&value)
}

/// Serializes one decoded document: the IR, the decode report, and source
/// fidelity. A decode error is frozen too: refusing a document is
/// contract-relevant behavior, so this never panics on codec output.
fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match StepCodec::default()
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
    {
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

/// Decodes `bytes`, then re-encodes it under three target requests.
///
/// `fidelity: None` forces synthesis on every arm, so the output is what the
/// writer builds from neutral IR rather than a replay of the source.
///
/// `inherit` resolves the target from the source declaration and is the arm
/// that varies per fixture. `explicit_default` and `explicit_ap242_e3` name a
/// catalog row outright, so they pin that the written `FILE_SCHEMA` and the
/// reported `ExportReport.target` agree no matter what the source said.
/// Decode and encode refusals freeze as `decode_error` / `encode_error`; a
/// refusal is contract-relevant behavior, so this never panics on codec
/// output.
///
/// The writer reads no clock: `StepWriteOptions::default()` stamps the fixed
/// `1970-01-01T00:00:00` into `FILE_NAME`, so no arm needs elision.
fn encode_snapshot(bytes: &[u8]) -> String {
    let decoded = match StepCodec::default()
        .decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
    {
        Ok(result) => result,
        Err(error) => {
            return snapshot_text(&serde_json::json!({ "decode_error": error.to_string() }))
        }
    };
    let value = serde_json::json!({
        "inherit": encode_arm(&decoded, TargetRequest::Inherit),
        "explicit_default": encode_arm(
            &decoded,
            TargetRequest::Explicit(StepSchema::Ap214.descriptor().id.as_str()),
        ),
        "explicit_ap242_e3": encode_arm(
            &decoded,
            TargetRequest::Explicit(StepSchema::Ap242Edition3.descriptor().id.as_str()),
        ),
    });
    snapshot_text(&value)
}

/// One encode arm: the export report and the bytes the writer produced.
fn encode_arm(
    decoded: &cadmpeg_ir::codec::DecodeResult,
    request: TargetRequest<'_>,
) -> serde_json::Value {
    let outcome = Encoder::plan(
        &StepCodec::default(),
        EncodeInput::new(decoded.ir(), None),
        request,
    )
    .and_then(|plan| {
        let mut produced = Vec::new();
        plan.write_to(&mut produced)
            .map(|report| (report, produced))
    });
    match outcome {
        Ok((report, produced)) => serde_json::json!({
            "report": report,
            "output": String::from_utf8_lossy(&produced),
        }),
        Err(error) => serde_json::json!({ "encode_error": error.to_string() }),
    }
}

/// Every committed golden still matches what the codec produces.
#[test]
fn golden_snapshots_hold() {
    harness().check(&branches());
    harness().check_inputs(&encode_inputs(), &encode_branch());
}

/// Putting the same bytes through a branch twice produces identical text.
#[test]
fn golden_output_is_deterministic() {
    harness().check_determinism(&branches());
    harness().check_determinism_inputs(&encode_inputs(), &encode_branch());
}

/// A geometry-depth decode agrees with the full decode outside the design
/// arenas. This codec has no geometry-depth cut, so every input delivers full.
#[test]
fn geometry_depth_agrees_with_full_depth() {
    let inputs = harness().fixture_inputs();
    assert!(!inputs.is_empty(), "no STEP fixtures to compare");
    let mut summary = cadmpeg_test_support::depth::DepthSummary::default();
    for input in &inputs {
        let compared = cadmpeg_test_support::depth::assert_geometry_agrees_with_full_with(
            &StepCodec::default(),
            std::slice::from_ref(input),
            |result| assert_fixture_source_content(&input.0, result),
        );
        summary.inputs += compared.inputs;
        summary.cut += compared.cut;
        summary.refused += compared.refused;
    }
    assert_eq!(summary.cut, 0, "{summary:?}");
}

/// Independent fixture expectations: unknown DATA types and the unsupported
/// `NULL_STYLE` assignment have no typed document record. Header/signature
/// retention and warnings about typed products or meshes do not add content.
fn assert_fixture_source_content(name: &str, result: &cadmpeg_ir::codec::DecodeResult) {
    use cadmpeg_ir::{ArenaClass, ArenaClasses};

    let expected_untyped: &[&str] = match name {
        // DATA #2 is EXAMPLE_RECORD; PRODUCT #1 is typed. SIGNATURE is
        // container evidence and does not carry an omitted model class.
        "ap242_ed3_sections" => &["step:data:example_record#2"],
        // #42 applies NULL_STYLE, which the style decoder does not admit;
        // its referring STYLED_ITEM #43 is also retained without a typed value.
        "ap242_geometry" => &[
            "step:data:presentation_style_assignment#42",
            "step:data:styled_item#43",
        ],
        // Both DATA records use fixture-only, unsupported entity types.
        "ap242_minimal" => &["step:data:example_record#1", "step:data:opaque_target#2"],
        "strings" => &["step:data:example_strings#1"],
        "ap203_sheet"
        | "ap214_sheet"
        | "ap242_assembly"
        | "ap242_conversion_units"
        | "ap242_degree_cone"
        | "ap242_external_documents"
        | "ap242_geometric_set"
        | "ap242_mapped_assembly"
        | "ap242_occurrence_mapped_assembly"
        | "ap242_presentation_pmi"
        | "ap242_semantic_pmi"
        | "ap242_tessellation"
        | "ap242_vertex_loop"
        | "complex_instance"
        | "noncanonical_solid_angle" => &[],
        _ => panic!("{name}: declare the fixture's typed/opaque source expectation"),
    };
    let expected_classes = if expected_untyped.is_empty() {
        ArenaClasses::new()
    } else {
        ArenaClasses::from([ArenaClass::Model])
    };
    assert_eq!(
        result.source_fidelity().unrepresented(),
        expected_classes,
        "{name}: omitted source content classes"
    );
    let actual_untyped = result
        .source_fidelity()
        .retained_records()
        .iter()
        .filter_map(|(id, record)| {
            let expected = if expected_untyped.contains(&id.as_str()) {
                ArenaClasses::from([ArenaClass::Model])
            } else {
                ArenaClasses::new()
            };
            assert_eq!(record.untyped(), &expected, "{name}: source record {id}");
            (!record.untyped().is_empty()).then_some(id.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual_untyped, expected_untyped,
        "{name}: opaque DATA records"
    );

    let model = &result.ir().model;
    if matches!(
        name,
        "ap242_assembly" | "ap242_mapped_assembly" | "ap242_occurrence_mapped_assembly"
    ) {
        // PRODUCT #3/#8 and PRODUCT_DEFINITION #6/#10 describe ROOT and
        // CHILD. SHAPE_REPRESENTATION #22/#23 contains only empty item
        // lists, placements, or mappings of those records, never topology.
        // The warning must not erase these typed products/occurrences.
        assert_eq!(
            model
                .product_definitions
                .iter()
                .map(|product| product.part_number.as_deref())
                .collect::<Vec<_>>(),
            [Some("ROOT"), Some("CHILD")],
            "{name}: typed products"
        );
        assert!(model
            .product_definitions
            .iter()
            .all(|product| product.bodies.is_empty()));
        // One root occurrence plus one per NEXT_ASSEMBLY_USAGE_OCCURRENCE:
        // #12 in both first fixtures, and #12/#13 in the third.
        let expected_occurrences = if name == "ap242_occurrence_mapped_assembly" {
            3
        } else {
            2
        };
        assert_eq!(
            model.occurrences.len(),
            expected_occurrences,
            "{name}: assembly occurrences"
        );
    }
    if name == "ap242_tessellation" {
        // COMPLEX_TRIANGULATED_FACE #7 references COORDINATES_LIST #6 and
        // carries a four-vertex strip and a three-vertex fan. Its absent
        // exact body container leaves the admitted mesh detached, not opaque.
        let mesh = model
            .tessellations
            .iter()
            .find(|mesh| mesh.id.as_str() == "step:tessellation:mesh#7")
            .expect("typed complex tessellated face #7");
        assert!(mesh.body.is_none());
        assert_eq!(mesh.vertices().len(), 4);
        assert_eq!(mesh.triangles(), [[0, 1, 2], [2, 1, 3], [0, 1, 3]]);
    }
}
