// SPDX-License-Identifier: Apache-2.0
//! Generate deterministic, conservative NX capability-gate evidence.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use cadmpeg_codec_nx::NxCodec;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};
use cadmpeg_ir::report::LossCategory;
use cadmpeg_ir::{CadIr, Severity};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Serialize)]
struct Profile {
    #[serde(rename = "profile_version")]
    version: u32,
    format: &'static str,
    fixtures: Vec<FixtureEvidence>,
    totals: EntityCounts,
    loss_codes: BTreeMap<String, usize>,
    rederivation_boundaries: Vec<RederivationBoundaryCount>,
    gates: Vec<Gate>,
    highest_passing_gate: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FixtureEvidence {
    filename: String,
    deterministic: bool,
    native_namespace_version: Option<u32>,
    entities: EntityCounts,
    losses: BTreeMap<LossCategory, usize>,
    loss_codes: BTreeMap<String, usize>,
    validation_errors: usize,
    all_bodies_colored: bool,
    all_faces_colored: bool,
    /// Neutral feature evaluation evidence for the saved current-body census.
    rederivation: VerificationStatus,
    /// First exact semantic boundary reached by neutral body-census replay.
    rederivation_boundary: Option<RederivationBoundary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum VerificationStatus {
    Missing,
    Verified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RederivationBoundary {
    feature: Option<String>,
    feature_name: Option<String>,
    feature_family: Option<String>,
    feature_ordinal: Option<u64>,
    reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RederivationBoundaryCount {
    reason: String,
    feature_family: Option<String>,
    fixtures: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct EntityCounts {
    assets: usize,
    bodies: usize,
    faces: usize,
    edges: usize,
    vertices: usize,
    features: usize,
    sketches: usize,
    sketch_entities: usize,
    sketch_constraints: usize,
    parameters: usize,
    configurations: usize,
}

impl EntityCounts {
    fn from_ir(ir: &CadIr) -> Self {
        Self {
            assets: ir.model.assets.len(),
            bodies: ir.model.bodies.len(),
            faces: ir.model.faces.len(),
            edges: ir.model.edges.len(),
            vertices: ir.model.vertices.len(),
            features: ir.model.features.len(),
            sketches: ir.model.sketches.len(),
            sketch_entities: ir.model.sketch_entities.len(),
            sketch_constraints: ir.model.sketch_constraints.len(),
            parameters: ir.model.parameters.len(),
            configurations: ir.model.configurations.len(),
        }
    }

    fn add(&mut self, other: &Self) {
        self.assets += other.assets;
        self.bodies += other.bodies;
        self.faces += other.faces;
        self.edges += other.edges;
        self.vertices += other.vertices;
        self.features += other.features;
        self.sketches += other.sketches;
        self.sketch_entities += other.sketch_entities;
        self.sketch_constraints += other.sketch_constraints;
        self.parameters += other.parameters;
        self.configurations += other.configurations;
    }
}

#[derive(Debug, Serialize)]
struct Gate {
    level: String,
    passed: bool,
    assertions: Vec<Assertion>,
}

#[derive(Debug, Serialize)]
struct Assertion {
    id: &'static str,
    passed: bool,
    observed: String,
    required: &'static str,
}

#[derive(Debug, Serialize, Deserialize)]
struct DecodedFixtureEvidence {
    canonical_sha256: String,
    native_namespace_version: Option<u32>,
    entities: EntityCounts,
    losses: BTreeMap<LossCategory, usize>,
    loss_codes: BTreeMap<String, usize>,
    validation_errors: usize,
    all_bodies_colored: bool,
    all_faces_colored: bool,
    rederivation: VerificationStatus,
    rederivation_boundary: Option<RederivationBoundary>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let first_argument = arguments
        .next()
        .ok_or("usage: nx-profile FIXTURE_DIRECTORY OUTPUT_JSON")?;
    if first_argument == OsStr::new("--decode-fixture") {
        let path = arguments
            .next()
            .map(PathBuf::from)
            .ok_or("usage: nx-profile --decode-fixture INPUT_PRT")?;
        if arguments.next().is_some() {
            return Err("usage: nx-profile --decode-fixture INPUT_PRT".into());
        }
        println!("{}", serde_json::to_string(&decode_fixture(&path)?)?);
        return Ok(());
    }
    let fixture_directory = PathBuf::from(first_argument);
    let output = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: nx-profile FIXTURE_DIRECTORY OUTPUT_JSON")?;
    if arguments.next().is_some() {
        return Err("usage: nx-profile FIXTURE_DIRECTORY OUTPUT_JSON".into());
    }

    let mut paths = fs::read_dir(fixture_directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("prt"))
        })
        .collect::<Vec<_>>();
    paths.sort();
    if paths.is_empty() {
        return Err("fixture directory contains no .prt files".into());
    }

    let mut fixtures = Vec::new();
    let mut totals = EntityCounts::default();
    let mut total_loss_codes = BTreeMap::new();
    for path in paths {
        let first = decode_fixture_in_worker(&path)?;
        let second = decode_fixture_in_worker(&path)?;
        let entities = first.entities;
        totals.add(&entities);
        for (code, count) in &first.loss_codes {
            *total_loss_codes.entry(code.clone()).or_insert(0) += count;
        }
        fixtures.push(FixtureEvidence {
            filename: path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("fixture filename is not UTF-8")?
                .to_string(),
            deterministic: first.canonical_sha256 == second.canonical_sha256,
            native_namespace_version: first.native_namespace_version,
            all_bodies_colored: first.all_bodies_colored,
            all_faces_colored: first.all_faces_colored,
            rederivation: first.rederivation,
            rederivation_boundary: first.rederivation_boundary,
            entities,
            losses: first.losses,
            loss_codes: first.loss_codes,
            validation_errors: first.validation_errors,
        });
    }

    let gates = capability_gates(&fixtures);
    let rederivation_boundaries = rederivation_boundary_counts(&fixtures);
    let highest_passing_gate = gates
        .iter()
        .take_while(|gate| gate.passed)
        .last()
        .map(|gate| gate.level.clone());
    let profile = Profile {
        version: 6,
        format: "nx",
        fixtures,
        totals,
        loss_codes: total_loss_codes,
        rederivation_boundaries,
        gates,
        highest_passing_gate,
    };
    let mut json = serde_json::to_string_pretty(&profile)?;
    json.push('\n');
    fs::write(output, json)?;
    Ok(())
}

fn rederivation_boundary_counts(fixtures: &[FixtureEvidence]) -> Vec<RederivationBoundaryCount> {
    let mut counts = BTreeMap::<(String, Option<String>), usize>::new();
    for boundary in fixtures
        .iter()
        .filter_map(|fixture| fixture.rederivation_boundary.as_ref())
    {
        *counts
            .entry((boundary.reason.clone(), boundary.feature_family.clone()))
            .or_default() += 1;
    }
    counts
        .into_iter()
        .map(
            |((reason, feature_family), fixtures)| RederivationBoundaryCount {
                reason,
                feature_family,
                fixtures,
            },
        )
        .collect()
}

fn decode_fixture(path: &Path) -> Result<DecodedFixtureEvidence, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    let decoded = NxCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default())?;
    let mut losses = BTreeMap::new();
    let mut loss_codes = BTreeMap::new();
    for loss in &decoded.report.losses {
        if loss.severity >= Severity::Warning {
            *losses.entry(loss.code.category()).or_insert(0) += 1;
            *loss_codes
                .entry(loss.code.as_str().to_string())
                .or_insert(0) += 1;
        }
    }
    let validation_errors = cadmpeg_ir::validate(&decoded.ir, Vec::new())
        .findings
        .iter()
        .filter(|finding| finding.severity >= Severity::Error)
        .count();
    let (rederivation, rederivation_boundary) = neutral_rederivation_evidence(&decoded.ir);
    Ok(DecodedFixtureEvidence {
        canonical_sha256: canonical_sha256(&decoded.ir)?,
        native_namespace_version: decoded
            .ir
            .native
            .namespace("nx")
            .map(|namespace| namespace.version),
        entities: EntityCounts::from_ir(&decoded.ir),
        losses,
        loss_codes,
        validation_errors,
        all_bodies_colored: !decoded.ir.model.bodies.is_empty()
            && decoded
                .ir
                .model
                .bodies
                .iter()
                .all(|body| body.color.is_some()),
        all_faces_colored: !decoded.ir.model.faces.is_empty()
            && decoded
                .ir
                .model
                .faces
                .iter()
                .all(|face| face.color.is_some()),
        rederivation,
        rederivation_boundary,
    })
}

/// Evaluate the admitted exact body-identity effects of neutral NX history.
fn neutral_rederivation_evidence(ir: &CadIr) -> (VerificationStatus, Option<RederivationBoundary>) {
    use cadmpeg_codec_nx::evaluation::{BodyCensusEvaluation, UnsupportedBodyCensusReason};

    match cadmpeg_codec_nx::evaluation::evaluate_saved_body_census(ir) {
        BodyCensusEvaluation::Verified { .. } => (VerificationStatus::Verified, None),
        BodyCensusEvaluation::Mismatch { .. } => (
            VerificationStatus::Missing,
            Some(RederivationBoundary {
                feature: None,
                feature_name: None,
                feature_family: None,
                feature_ordinal: None,
                reason: "saved_body_census_mismatch".to_string(),
            }),
        ),
        BodyCensusEvaluation::Unsupported { feature, reason } => {
            let boundary_feature = feature.as_ref().and_then(|id| {
                ir.model
                    .features
                    .iter()
                    .find(|candidate| candidate.id == *id)
            });
            let feature_name = boundary_feature.and_then(|feature| feature.name.clone());
            let feature_family = boundary_feature.and_then(|feature| {
                serde_json::to_value(&feature.definition)
                    .ok()?
                    .get("definition")?
                    .as_str()
                    .map(str::to_string)
            });
            let feature_ordinal = boundary_feature.map(|feature| feature.ordinal);
            let reason = match reason {
                UnsupportedBodyCensusReason::UnresolvedSuppression => "unresolved_suppression",
                UnsupportedBodyCensusReason::UnsupportedFeatureDefinition => {
                    "unsupported_feature_definition"
                }
                UnsupportedBodyCensusReason::IncompleteFeatureDefinition => {
                    "incomplete_feature_definition"
                }
                UnsupportedBodyCensusReason::InvalidOutputLineage => "invalid_output_lineage",
                UnsupportedBodyCensusReason::InvalidHistoryOrder => "invalid_history_order",
                UnsupportedBodyCensusReason::ConfigurationEvaluation => "configuration_evaluation",
                _ => "unknown_evaluation_boundary",
            };
            (
                VerificationStatus::Missing,
                Some(RederivationBoundary {
                    feature: feature.map(|id| id.0),
                    feature_name,
                    feature_family,
                    feature_ordinal,
                    reason: reason.to_string(),
                }),
            )
        }
        _ => (
            VerificationStatus::Missing,
            Some(RederivationBoundary {
                feature: None,
                feature_name: None,
                feature_family: None,
                feature_ordinal: None,
                reason: "unknown_evaluation_boundary".to_string(),
            }),
        ),
    }
}

fn canonical_sha256(ir: &CadIr) -> Result<String, serde_json::Error> {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    struct Sha256Writer(Sha256);

    impl Write for Sha256Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut writer = Sha256Writer(Sha256::new());
    serde_json::to_writer_pretty(&mut writer, ir)?;
    let digest = writer.0.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(encoded)
}

fn decode_fixture_in_worker(
    path: &Path,
) -> Result<DecodedFixtureEvidence, Box<dyn std::error::Error>> {
    let output = Command::new(env::current_exe()?)
        .arg("--decode-fixture")
        .arg(path)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("NX profile worker failed for {}: {stderr}", path.display()).into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn capability_gates(fixtures: &[FixtureEvidence]) -> Vec<Gate> {
    let deterministic = fixtures
        .iter()
        .filter(|fixture| fixture.deterministic)
        .count();
    let valid_topology = fixtures
        .iter()
        .filter(|fixture| {
            fixture.validation_errors == 0
                && fixture
                    .losses
                    .get(&LossCategory::Topology)
                    .copied()
                    .unwrap_or(0)
                    == 0
        })
        .count();
    let without = |category| {
        fixtures
            .iter()
            .filter(|fixture| fixture.losses.get(&category).copied().unwrap_or(0) == 0)
            .count()
    };
    let body_colors = fixtures
        .iter()
        .filter(|fixture| fixture.entities.bodies == 0 || fixture.all_bodies_colored)
        .count();
    let face_colors = fixtures
        .iter()
        .filter(|fixture| fixture.entities.faces == 0 || fixture.all_faces_colored)
        .count();
    let rederivation_verified = fixtures
        .iter()
        .filter(|fixture| fixture.rederivation == VerificationStatus::Verified)
        .count();
    let total = fixtures.len();
    let assertion = |id, count, required| Assertion {
        id,
        passed: count == total,
        observed: format!("{count}/{total} fixtures"),
        required,
    };
    let rows = vec![
        (
            "L0",
            vec![assertion(
                "deterministic_decode",
                deterministic,
                "every fixture decodes deterministically",
            )],
        ),
        (
            "L1",
            vec![assertion(
                "closed_container_navigation",
                without(LossCategory::Other),
                "no fixture reports a container or unsupported-content loss",
            )],
        ),
        (
            "L2",
            vec![assertion(
                "complete_geometry",
                without(LossCategory::Geometry),
                "no fixture reports a geometry loss",
            )],
        ),
        (
            "L3",
            vec![assertion(
                "valid_connected_topology",
                valid_topology,
                "every fixture validates and reports no topology loss",
            )],
        ),
        (
            "L4",
            vec![assertion(
                "complete_design_records",
                without(LossCategory::DesignIntent),
                "no fixture reports a design-intent loss",
            )],
        ),
        (
            "L5",
            vec![
                assertion(
                    "body_appearance",
                    body_colors,
                    "every decoded body has a color",
                ),
                assertion(
                    "face_appearance",
                    face_colors,
                    "every decoded face has a color",
                ),
                assertion(
                    "complete_attributes",
                    without(LossCategory::Attribute),
                    "no fixture reports an attribute loss",
                ),
                assertion(
                    "complete_materials",
                    without(LossCategory::Material),
                    "no fixture reports a material loss",
                ),
            ],
        ),
        (
            "L6",
            vec![
                assertion(
                    "design_domain_loss_empty",
                    without(LossCategory::DesignIntent),
                    "no fixture reports a design-intent loss",
                ),
                assertion(
                    "saved_body_census_rederived",
                    rederivation_verified,
                    "neutral feature evaluation reproduces every saved current-body census",
                ),
            ],
        ),
    ];
    let mut lower_passed = true;
    rows.into_iter()
        .map(|(level, assertions)| {
            let passed = lower_passed && assertions.iter().all(|assertion| assertion.passed);
            lower_passed = passed;
            Gate {
                level: level.to_string(),
                passed,
                assertions,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::features::{FeatureDefinition, Length};
    use cadmpeg_ir::ids::BodyId;

    fn fixture() -> FixtureEvidence {
        FixtureEvidence {
            filename: "fixture.prt".to_string(),
            deterministic: true,
            native_namespace_version: Some(181),
            entities: EntityCounts::default(),
            losses: BTreeMap::new(),
            loss_codes: BTreeMap::new(),
            validation_errors: 0,
            all_bodies_colored: false,
            all_faces_colored: false,
            rederivation: VerificationStatus::Verified,
            rederivation_boundary: None,
        }
    }

    #[test]
    fn streaming_canonical_hash_matches_the_canonical_document() {
        let ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        assert_eq!(
            canonical_sha256(&ir).expect("test IR must serialize through the profile writer"),
            cadmpeg_ir::hash::sha256_hex(
                ir.to_canonical_json()
                    .expect("test IR must serialize canonically")
                    .as_bytes(),
            )
        );
    }

    #[test]
    fn empty_neutral_history_rederives_the_empty_saved_body_census() {
        let ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        assert_eq!(
            neutral_rederivation_evidence(&ir),
            (VerificationStatus::Verified, None)
        );
    }

    #[test]
    fn complete_block_rederives_its_saved_body_identity() {
        use cadmpeg_ir::features::{Feature, FeatureId};
        use cadmpeg_ir::topology::{Body, BodyKind};

        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        let body = BodyId("body".to_string());
        ir.model.bodies.push(Body {
            id: body.clone(),
            kind: BodyKind::Solid,
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        ir.model.features.push(Feature {
            id: FeatureId("block".to_string()),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: vec![body],
            definition: FeatureDefinition::Block {
                dimensions: Some([Length(1.0), Length(2.0), Length(3.0)]),
                placement: Some(cadmpeg_ir::transform::Transform::identity()),
                op: cadmpeg_ir::features::BooleanOp::NewBody,
            },
            native_ref: None,
        });

        assert_eq!(
            neutral_rederivation_evidence(&ir),
            (VerificationStatus::Verified, None)
        );
    }

    #[test]
    fn rederivation_boundary_identifies_feature_family_and_history_position() {
        use cadmpeg_ir::features::{Feature, FeatureId};

        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        ir.model.features.push(Feature {
            id: FeatureId("block".to_string()),
            ordinal: 17,
            name: Some("BLOCK".to_string()),
            suppressed: Some(false),
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: vec![BodyId("body".to_string())],
            definition: FeatureDefinition::Block {
                dimensions: None,
                placement: None,
                op: cadmpeg_ir::features::BooleanOp::Unresolved,
            },
            native_ref: None,
        });

        let (status, boundary) = neutral_rederivation_evidence(&ir);
        assert_eq!(status, VerificationStatus::Missing);
        assert_eq!(
            boundary,
            Some(RederivationBoundary {
                feature: Some("block".to_string()),
                feature_name: Some("BLOCK".to_string()),
                feature_family: Some("block".to_string()),
                feature_ordinal: Some(17),
                reason: "incomplete_feature_definition".to_string(),
            })
        );
    }

    #[test]
    fn rederivation_boundary_census_groups_reason_and_feature_family() {
        let boundary = |reason: &str, family: Option<&str>| RederivationBoundary {
            feature: None,
            feature_name: None,
            feature_family: family.map(str::to_string),
            feature_ordinal: None,
            reason: reason.to_string(),
        };
        let mut fixtures = [fixture(), fixture(), fixture()];
        fixtures[0].rederivation = VerificationStatus::Missing;
        fixtures[0].rederivation_boundary =
            Some(boundary("incomplete_feature_definition", Some("hole")));
        fixtures[1].rederivation = VerificationStatus::Missing;
        fixtures[1].rederivation_boundary =
            Some(boundary("incomplete_feature_definition", Some("hole")));
        fixtures[2].rederivation = VerificationStatus::Missing;
        fixtures[2].rederivation_boundary = Some(boundary("configuration_evaluation", None));

        assert_eq!(
            rederivation_boundary_counts(&fixtures),
            vec![
                RederivationBoundaryCount {
                    reason: "configuration_evaluation".to_string(),
                    feature_family: None,
                    fixtures: 1,
                },
                RederivationBoundaryCount {
                    reason: "incomplete_feature_definition".to_string(),
                    feature_family: Some("hole".to_string()),
                    fixtures: 2,
                },
            ]
        );
    }

    #[test]
    fn unresolved_body_neutral_state_does_not_block_rederivation() {
        use cadmpeg_ir::features::{Feature, FeatureId, FeatureTreeNodeRole};

        let mut ir = CadIr::empty(cadmpeg_ir::units::Units::default());
        ir.model.features.push(Feature {
            id: FeatureId("feature".to_string()),
            ordinal: 0,
            name: None,
            suppressed: None,
            parent: None,
            dependencies: Vec::new(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: Vec::new(),
            outputs: Vec::new(),
            definition: FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::History,
                children: Vec::new(),
                active_child: None,
            },
            native_ref: None,
        });

        assert_eq!(
            neutral_rederivation_evidence(&ir),
            (VerificationStatus::Verified, None)
        );
    }

    #[test]
    fn gates_are_cumulative() {
        let mut evidence = fixture();
        evidence.losses.insert(LossCategory::Geometry, 1);
        let gates = capability_gates(&[evidence]);

        assert!(gates[0].passed);
        assert!(gates[1].passed);
        assert!(!gates[2].passed);
        assert!(gates[3..].iter().all(|gate| !gate.passed));
    }

    #[test]
    fn topology_gate_requires_validation_and_loss_closure_in_each_fixture() {
        let mut invalid = fixture();
        invalid.validation_errors = 1;
        let mut lossy = fixture();
        lossy.losses.insert(LossCategory::Topology, 1);

        let gates = capability_gates(&[invalid, lossy]);
        let assertion = &gates[3].assertions[0];
        assert_eq!(assertion.observed, "0/2 fixtures");
        assert!(!gates[3].passed);
    }

    #[test]
    fn product_losses_do_not_fail_the_l0_through_l6_profile() {
        let mut evidence = fixture();
        evidence.losses.insert(LossCategory::Product, 2);
        let gates = capability_gates(&[evidence]);

        assert!(gates.iter().all(|gate| gate.passed));
    }

    #[test]
    fn l6_requires_rederivation_evidence_distinct_from_design_loss_closure() {
        let mut evidence = fixture();
        evidence.rederivation = VerificationStatus::Missing;
        let gates = capability_gates(&[evidence]);

        assert!(gates[..6].iter().all(|gate| gate.passed));
        assert!(!gates[6].passed);
        assert!(gates[6].assertions[0].passed);
        assert_eq!(gates[6].assertions[1].id, "saved_body_census_rederived");
        assert!(!gates[6].assertions[1].passed);
    }
}
