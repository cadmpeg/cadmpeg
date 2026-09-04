// SPDX-License-Identifier: Apache-2.0
//! Generate deterministic, conservative NX capability-gate evidence.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use cadmpeg_codec_nx::{saved_body_census_evidence, BodyCensusEvidence, NxCodec};
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::LossCategory;
use cadmpeg_ir::topology::Color;
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
    loss_details: BTreeMap<String, usize>,
    rederivation_boundaries: Vec<RederivationBoundaryCount>,
    gates: Vec<Gate>,
    highest_passing_gate: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FixtureEvidence {
    filename: String,
    status: DecodeStatus,
    deterministic: bool,
    native_namespace_version: Option<u32>,
    entities: EntityCounts,
    losses: BTreeMap<LossCategory, usize>,
    loss_codes: BTreeMap<String, usize>,
    loss_details: BTreeMap<String, usize>,
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
    Inapplicable,
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

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct EntityCounts {
    assets: usize,
    bodies: usize,
    faces: usize,
    edges: usize,
    vertices: usize,
    features: usize,
    feature_result_topologies: usize,
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
            feature_result_topologies: ir.model.feature_result_topologies.len(),
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
        self.feature_result_topologies += other.feature_result_topologies;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DecodedFixtureEvidence {
    canonical_sha256: String,
    native_namespace_version: Option<u32>,
    entities: EntityCounts,
    losses: BTreeMap<LossCategory, usize>,
    loss_codes: BTreeMap<String, usize>,
    loss_details: BTreeMap<String, usize>,
    validation_errors: usize,
    all_bodies_colored: bool,
    all_faces_colored: bool,
    rederivation: VerificationStatus,
    rederivation_boundary: Option<RederivationBoundary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DecodeStatus {
    /// Both bounded worker decodes completed.
    Complete,
    /// At least one bounded worker reached its per-file timeout.
    TimedOut,
    /// At least one bounded worker failed before producing evidence.
    Failed,
}

#[derive(Debug)]
enum WorkerFailure {
    TimedOut,
    Failed(String),
}

impl std::fmt::Display for WorkerFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedOut => formatter.write_str("worker timed out"),
            Self::Failed(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for WorkerFailure {}

// A large admitted part can require several minutes for one complete decode.
// Keep the bound per worker so one pathological file cannot stall the profile,
// while allowing the largest supported object-model sections to finish.
const WORKER_TIMEOUT: Duration = Duration::from_secs(600);
const NX_LOSS_NAMESPACE: &str = "nx";
const EXTERNAL_ASSEMBLY_LOSS_CODE: &str = "assembly.components-external";

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

    let paths = fixture_paths(&fixture_directory)?;
    if paths.is_empty() {
        return Err("fixture directory contains no .prt files".into());
    }

    let mut fixtures = Vec::new();
    let mut totals = EntityCounts::default();
    let mut total_loss_codes = BTreeMap::new();
    let mut total_loss_details = BTreeMap::new();
    for path in paths {
        let first = decode_fixture_in_worker(&path);
        let second = decode_fixture_in_worker(&path);
        let status = decode_status(&first, &second);
        let deterministic = match (first.as_ref(), second.as_ref()) {
            (Ok(first), Ok(second)) => first.canonical_sha256 == second.canonical_sha256,
            _ => false,
        };
        let filename = path
            .strip_prefix(&fixture_directory)
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .into_owned();
        if let Some(decoded) = first.as_ref().ok().or_else(|| second.as_ref().ok()) {
            add_totals(
                decoded,
                &mut totals,
                &mut total_loss_codes,
                &mut total_loss_details,
            );
            fixtures.push(fixture_evidence(filename, status, deterministic, decoded));
        } else {
            fixtures.push(failed_fixture_evidence(filename, status));
        }
    }

    let gates = capability_gates(&fixtures);
    let rederivation_boundaries = rederivation_boundary_counts(&fixtures);
    let highest_passing_gate = gates
        .iter()
        .take_while(|gate| gate.passed)
        .last()
        .map(|gate| gate.level.clone());
    let profile = Profile {
        version: 11,
        format: "nx",
        fixtures,
        totals,
        loss_codes: total_loss_codes,
        loss_details: total_loss_details,
        rederivation_boundaries,
        gates,
        highest_passing_gate,
    };
    let mut json = serde_json::to_string_pretty(&profile)?;
    json.push('\n');
    fs::write(output, json)?;
    Ok(())
}

fn fixture_paths(root: &Path) -> io::Result<Vec<PathBuf>> {
    fn visit(directory: &Path, paths: &mut Vec<PathBuf>) -> io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                visit(&path, paths)?;
            } else if file_type.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("prt"))
            {
                paths.push(path);
            }
        }
        Ok(())
    }

    let mut paths = Vec::new();
    visit(root, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn decode_status(
    first: &Result<DecodedFixtureEvidence, WorkerFailure>,
    second: &Result<DecodedFixtureEvidence, WorkerFailure>,
) -> DecodeStatus {
    if first.is_ok() && second.is_ok() {
        DecodeStatus::Complete
    } else if [first, second]
        .into_iter()
        .any(|result| matches!(result, Err(WorkerFailure::TimedOut)))
    {
        DecodeStatus::TimedOut
    } else {
        DecodeStatus::Failed
    }
}

fn add_totals(
    decoded: &DecodedFixtureEvidence,
    totals: &mut EntityCounts,
    total_loss_codes: &mut BTreeMap<String, usize>,
    total_loss_details: &mut BTreeMap<String, usize>,
) {
    totals.add(&decoded.entities);
    for (code, count) in &decoded.loss_codes {
        *total_loss_codes.entry(code.clone()).or_insert(0) += count;
    }
    for (detail, count) in &decoded.loss_details {
        *total_loss_details.entry(detail.clone()).or_insert(0) += count;
    }
}

fn fixture_evidence(
    filename: String,
    status: DecodeStatus,
    deterministic: bool,
    decoded: &DecodedFixtureEvidence,
) -> FixtureEvidence {
    FixtureEvidence {
        filename,
        status,
        deterministic,
        native_namespace_version: decoded.native_namespace_version,
        all_bodies_colored: decoded.all_bodies_colored,
        all_faces_colored: decoded.all_faces_colored,
        rederivation: decoded.rederivation,
        rederivation_boundary: decoded.rederivation_boundary.clone(),
        entities: decoded.entities.clone(),
        losses: decoded.losses.clone(),
        loss_codes: decoded.loss_codes.clone(),
        loss_details: decoded.loss_details.clone(),
        validation_errors: decoded.validation_errors,
    }
}

fn failed_fixture_evidence(filename: String, status: DecodeStatus) -> FixtureEvidence {
    let reason = match status {
        DecodeStatus::TimedOut => "profile_worker_timeout",
        DecodeStatus::Failed => "profile_worker_failure",
        DecodeStatus::Complete => unreachable!("completed status has decoded evidence"),
    };
    FixtureEvidence {
        filename,
        status,
        deterministic: false,
        native_namespace_version: None,
        entities: EntityCounts::default(),
        losses: BTreeMap::new(),
        loss_codes: BTreeMap::new(),
        loss_details: BTreeMap::new(),
        validation_errors: 1,
        all_bodies_colored: false,
        all_faces_colored: false,
        rederivation: VerificationStatus::Missing,
        rederivation_boundary: Some(RederivationBoundary {
            feature: None,
            feature_name: None,
            feature_family: None,
            feature_ordinal: None,
            reason: reason.to_string(),
        }),
    }
}

fn rederivation_boundary_counts(fixtures: &[FixtureEvidence]) -> Vec<RederivationBoundaryCount> {
    let mut counts = BTreeMap::<(String, Option<String>), usize>::new();
    for boundary in fixtures
        .iter()
        .filter(|fixture| fixture.rederivation != VerificationStatus::Inapplicable)
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
    let mut loss_details = BTreeMap::new();
    for loss in &decoded.report().losses {
        if loss.severity >= Severity::Warning {
            *losses.entry(loss.code.category()).or_insert(0) += 1;
            *loss_codes.entry(loss.code.as_str()).or_insert(0) += 1;
            *loss_details.entry(loss.message.clone()).or_insert(0) += 1;
        }
    }
    let validation_errors = cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new())
        .findings
        .iter()
        .filter(|finding| finding.severity >= Severity::Error)
        .count();
    let part_design_applicable = !decoded
        .report()
        .losses
        .iter()
        .any(|loss| is_external_assembly_loss(&loss.code));
    let (rederivation, rederivation_boundary) = if part_design_applicable {
        neutral_rederivation_evidence(decoded.ir())
    } else {
        (VerificationStatus::Inapplicable, None)
    };
    Ok(DecodedFixtureEvidence {
        canonical_sha256: canonical_sha256(decoded.ir())?,
        native_namespace_version: decoded
            .ir()
            .native
            .namespace("nx")
            .map(cadmpeg_ir::NativeNamespace::version),
        entities: EntityCounts::from_ir(decoded.ir()),
        losses,
        loss_codes,
        loss_details,
        validation_errors,
        all_bodies_colored: !decoded.ir().model.bodies.is_empty()
            && decoded.ir().model.bodies.iter().all(|body| {
                has_effective_color(
                    decoded.ir(),
                    body.color,
                    &AppearanceTarget::Body(body.id.clone()),
                )
            }),
        all_faces_colored: !decoded.ir().model.faces.is_empty()
            && decoded.ir().model.faces.iter().all(|face| {
                has_effective_color(
                    decoded.ir(),
                    face.color,
                    &AppearanceTarget::Face(face.id.clone()),
                )
            }),
        rederivation,
        rederivation_boundary,
    })
}

fn is_external_assembly_loss(code: &cadmpeg_ir::report::LossKind) -> bool {
    code.namespace() == NX_LOSS_NAMESPACE && code.local_code() == EXTERNAL_ASSEMBLY_LOSS_CODE
}

/// Return the target's effective color under the neutral appearance contract.
///
/// An absent direct color is a complete no-assignment state when no
/// topology-targeted binding exists. A direct color is sufficient when no
/// topology-targeted binding exists. If a binding exists, it must be the sole
/// binding and agree with the direct color; without a direct color it must
/// resolve to exactly one appearance with a normalized base color.
/// Source-carrier bindings and ambiguous target bindings do not supply a
/// topology color.
fn has_effective_color(ir: &CadIr, direct_color: Option<Color>, target: &AppearanceTarget) -> bool {
    let bindings = ir
        .model
        .appearance_bindings
        .iter()
        .filter(|binding| &binding.target == target);
    let bindings = bindings.collect::<Vec<_>>();
    if bindings.len() > 1 {
        return false;
    }

    let bound_color = bindings.first().and_then(|binding| {
        let mut appearances = ir
            .model
            .appearances
            .iter()
            .filter(|appearance| appearance.id == binding.appearance);
        let appearance = appearances.next()?;
        if appearances.next().is_some() {
            return None;
        }
        appearance
            .base_color
            .filter(|color| normalized_color(*color))
    });

    // A body appearance is the base for every owned face that has no direct
    // face color or face-scoped binding. Require unique topology ownership so
    // an authored body appearance cannot inherit through an ambiguous face.
    if direct_color.is_none() && bindings.is_empty() {
        if let AppearanceTarget::Face(face_id) = target {
            let Some(body) = unique_face_body(ir, face_id) else {
                let body_appearance_exists = ir.model.bodies.iter().any(|body| {
                    body.color.is_some()
                        || ir.model.appearance_bindings.iter().any(|binding| {
                            binding.target == AppearanceTarget::Body(body.id.clone())
                        })
                });
                return !body_appearance_exists;
            };
            let body_target = AppearanceTarget::Body(body.id.clone());
            let body_appearance_exists = body.color.is_some()
                || ir
                    .model
                    .appearance_bindings
                    .iter()
                    .any(|binding| binding.target == body_target);
            if body_appearance_exists {
                return has_effective_color(ir, body.color, &body_target);
            }
        }
        return true;
    }

    match direct_color {
        Some(color) => match bindings.first() {
            None => normalized_color(color),
            Some(_) => bound_color.is_some_and(|bound| normalized_color(color) && bound == color),
        },
        None => bound_color.is_some(),
    }
}

fn unique_face_body<'a>(
    ir: &'a CadIr,
    face_id: &cadmpeg_ir::ids::FaceId,
) -> Option<&'a cadmpeg_ir::topology::Body> {
    let body_ids = ir
        .model
        .regions
        .iter()
        .filter_map(|region| {
            let owns_face = region.shells.iter().any(|shell_id| {
                ir.model
                    .shells
                    .iter()
                    .find(|shell| shell.id == *shell_id)
                    .is_some_and(|shell| shell.faces.iter().any(|face| face == face_id))
            });
            owns_face.then_some(region.body.clone())
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut body_ids = body_ids.into_iter();
    let body_id = body_ids.next()?;
    if body_ids.next().is_some() {
        return None;
    }
    ir.model.bodies.iter().find(|body| body.id == body_id)
}

fn normalized_color(color: Color) -> bool {
    [color.r, color.g, color.b, color.a]
        .into_iter()
        .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
}

/// Evaluate the admitted exact body-identity effects of neutral NX history.
fn neutral_rederivation_evidence(ir: &CadIr) -> (VerificationStatus, Option<RederivationBoundary>) {
    let BodyCensusEvidence {
        verified,
        reason,
        feature,
        feature_name,
        feature_family,
        feature_ordinal,
    } = saved_body_census_evidence(ir);
    if verified {
        (VerificationStatus::Verified, None)
    } else {
        (
            VerificationStatus::Missing,
            Some(RederivationBoundary {
                feature,
                feature_name,
                feature_family,
                feature_ordinal,
                reason: reason.unwrap_or_else(|| "unknown_evaluation_boundary".to_string()),
            }),
        )
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

fn decode_fixture_in_worker(path: &Path) -> Result<DecodedFixtureEvidence, WorkerFailure> {
    let worker =
        Command::new(env::current_exe().map_err(|error| WorkerFailure::Failed(error.to_string()))?)
            .arg("--decode-fixture")
            .arg(path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| WorkerFailure::Failed(error.to_string()))?;
    let output = wait_for_worker(worker)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(WorkerFailure::Failed(format!(
            "NX profile worker failed for {}: {stderr}",
            path.display()
        )));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| WorkerFailure::Failed(error.to_string()))
}

fn wait_for_worker(mut worker: Child) -> Result<Output, WorkerFailure> {
    let deadline = Instant::now() + WORKER_TIMEOUT;
    loop {
        if worker
            .try_wait()
            .map_err(|error| WorkerFailure::Failed(error.to_string()))?
            .is_some()
        {
            return worker
                .wait_with_output()
                .map_err(|error| WorkerFailure::Failed(error.to_string()));
        }
        if Instant::now() >= deadline {
            let _ = worker.kill();
            let _ = worker.wait();
            return Err(WorkerFailure::TimedOut);
        }
        thread::sleep(Duration::from_millis(20));
    }
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
    let part_design_total = fixtures
        .iter()
        .filter(|fixture| fixture.rederivation != VerificationStatus::Inapplicable)
        .count();
    let without_part_design_loss = fixtures
        .iter()
        .filter(|fixture| {
            fixture.rederivation != VerificationStatus::Inapplicable
                && fixture
                    .losses
                    .get(&LossCategory::DesignIntent)
                    .copied()
                    .unwrap_or(0)
                    == 0
        })
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
    let part_design_assertion = |id, count, required| Assertion {
        id,
        passed: count == part_design_total,
        observed: format!("{count}/{part_design_total} applicable fixtures"),
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
            vec![part_design_assertion(
                "complete_design_records",
                without_part_design_loss,
                "no applicable single-part fixture reports a design-intent loss",
            )],
        ),
        (
            "L5",
            vec![
                assertion(
                    "body_appearance",
                    body_colors,
                    "every decoded body has a valid color assignment or no authored color assignment",
                ),
                assertion(
                    "face_appearance",
                    face_colors,
                    "every decoded face has a valid color assignment or no authored color assignment",
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
                part_design_assertion(
                    "design_domain_loss_empty",
                    without_part_design_loss,
                    "no applicable single-part fixture reports a design-intent loss",
                ),
                part_design_assertion(
                    "saved_body_census_rederived",
                    rederivation_verified,
                    "neutral feature evaluation reproduces every applicable saved current-body census",
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
            status: DecodeStatus::Complete,
            deterministic: true,
            native_namespace_version: Some(181),
            entities: EntityCounts::default(),
            losses: BTreeMap::new(),
            loss_codes: BTreeMap::new(),
            loss_details: BTreeMap::new(),
            validation_errors: 0,
            all_bodies_colored: false,
            all_faces_colored: false,
            rederivation: VerificationStatus::Verified,
            rederivation_boundary: None,
        }
    }

    #[test]
    fn streaming_canonical_hash_matches_the_canonical_document() {
        let ir = CadIr::empty();
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
    fn external_assembly_applicability_uses_the_complete_local_loss_identity() {
        use cadmpeg_ir::report::{LossKind, LossTaxonomy};

        let external = LossKind::namespaced(
            NX_LOSS_NAMESPACE,
            EXTERNAL_ASSEMBLY_LOSS_CODE,
            LossTaxonomy::AssemblyComponentsExternal,
        );
        let wrong_namespace = LossKind::namespaced(
            "other",
            EXTERNAL_ASSEMBLY_LOSS_CODE,
            LossTaxonomy::AssemblyComponentsExternal,
        );
        let wrong_code = LossKind::namespaced(
            NX_LOSS_NAMESPACE,
            "assembly.other",
            LossTaxonomy::AssemblyComponentsExternal,
        );

        assert!(is_external_assembly_loss(&external));
        assert!(!is_external_assembly_loss(&wrong_namespace));
        assert!(!is_external_assembly_loss(&wrong_code));
    }

    #[test]
    fn empty_neutral_history_rederives_the_empty_saved_body_census() {
        let ir = CadIr::empty();
        assert_eq!(
            neutral_rederivation_evidence(&ir),
            (VerificationStatus::Verified, None)
        );
    }

    #[test]
    fn complete_block_rederives_its_saved_body_identity() {
        use cadmpeg_ir::features::{Feature, FeatureId};
        use cadmpeg_ir::topology::{Body, BodyKind};

        let mut ir = CadIr::empty();
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

        let mut ir = CadIr::empty();
        ir.model.features.push(Feature {
            id: FeatureId("block".to_string()),
            ordinal: 17,
            name: Some("BLOCK".to_string()),
            suppressed: Some(false),
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

        let mut ir = CadIr::empty();
        ir.model.features.push(Feature {
            id: FeatureId("feature".to_string()),
            ordinal: 0,
            name: None,
            suppressed: None,
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
    fn effective_color_accepts_a_unique_base_color_binding() {
        use cadmpeg_ir::appearance::{Appearance, AppearanceBinding};
        use cadmpeg_ir::ids::AppearanceId;

        let mut ir = CadIr::empty();
        let body = cadmpeg_ir::topology::Body {
            id: BodyId("body".to_string()),
            kind: cadmpeg_ir::topology::BodyKind::Solid,
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        };
        let appearance_id = AppearanceId("appearance".to_string());
        ir.model.bodies.push(body.clone());
        ir.model.appearances.push(Appearance {
            id: appearance_id.clone(),
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: Some(Color {
                r: 0.1,
                g: 0.2,
                b: 0.3,
                a: 1.0,
            }),
            properties: BTreeMap::new(),
            textures: Vec::new(),
        });
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: "binding".into(),
            target: AppearanceTarget::Body(body.id.clone()),
            appearance: appearance_id,
            source_entity_id: None,
            object_type: None,
            visible: None,
            channels: BTreeMap::new(),
        });

        assert!(has_effective_color(
            &ir,
            body.color,
            &AppearanceTarget::Body(body.id),
        ));

        ir.model.bodies[0].color = Some(Color {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            a: 1.0,
        });
        assert!(has_effective_color(
            &ir,
            ir.model.bodies[0].color,
            &AppearanceTarget::Body(ir.model.bodies[0].id.clone()),
        ));
        ir.model.appearances[0].base_color = None;
        assert!(!has_effective_color(
            &ir,
            ir.model.bodies[0].color,
            &AppearanceTarget::Body(ir.model.bodies[0].id.clone()),
        ));
        ir.model.appearances[0].base_color = Some(Color {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            a: 1.0,
        });
        ir.model.bodies[0].color = Some(Color {
            r: 0.9,
            g: 0.2,
            b: 0.3,
            a: 1.0,
        });
        assert!(!has_effective_color(
            &ir,
            ir.model.bodies[0].color,
            &AppearanceTarget::Body(ir.model.bodies[0].id.clone()),
        ));
    }

    #[test]
    fn effective_color_rejects_ambiguous_or_non_color_bindings() {
        use cadmpeg_ir::appearance::{Appearance, AppearanceBinding};
        use cadmpeg_ir::ids::AppearanceId;

        let mut ir = CadIr::empty();
        let body = cadmpeg_ir::topology::Body {
            id: BodyId("body".to_string()),
            kind: cadmpeg_ir::topology::BodyKind::Solid,
            regions: Vec::new(),
            transform: None,
            name: None,
            color: None,
            visible: None,
        };
        let appearance_id = AppearanceId("appearance".to_string());
        ir.model.bodies.push(body.clone());
        ir.model.appearances.push(Appearance {
            id: appearance_id.clone(),
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: None,
            properties: BTreeMap::new(),
            textures: Vec::new(),
        });
        let target = AppearanceTarget::Body(body.id.clone());
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: "binding-1".into(),
            target: target.clone(),
            appearance: appearance_id.clone(),
            source_entity_id: None,
            object_type: None,
            visible: None,
            channels: BTreeMap::new(),
        });
        assert!(!has_effective_color(&ir, None, &target));

        ir.model.appearances[0].base_color = Some(Color {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            a: 1.0,
        });
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: "binding-2".into(),
            target: target.clone(),
            appearance: appearance_id,
            source_entity_id: None,
            object_type: None,
            visible: None,
            channels: BTreeMap::new(),
        });
        assert!(!has_effective_color(&ir, None, &target));
    }

    #[test]
    fn effective_color_accepts_absent_color_without_an_assignment() {
        let ir = CadIr::empty();
        let target = AppearanceTarget::Body(BodyId("body".to_string()));

        assert!(has_effective_color(&ir, None, &target));
    }

    #[test]
    fn effective_color_requires_normalized_direct_color() {
        let ir = CadIr::empty();
        let target = AppearanceTarget::Body(BodyId("body".to_string()));
        assert!(!has_effective_color(
            &ir,
            Some(Color {
                r: 1.1,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }),
            &target,
        ));
        assert!(has_effective_color(
            &ir,
            Some(Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }),
            &target,
        ));
    }

    #[test]
    fn effective_face_color_inherits_unique_body_color() {
        let mut ir = cadmpeg_ir::examples::unit_cube();
        let body_color = Color {
            r: 0.2,
            g: 0.3,
            b: 0.4,
            a: 1.0,
        };
        ir.model.bodies[0].color = Some(body_color);

        assert!(ir.model.faces.iter().all(|face| {
            has_effective_color(&ir, face.color, &AppearanceTarget::Face(face.id.clone()))
        }));
    }

    #[test]
    fn product_losses_do_not_fail_the_l0_through_l6_profile() {
        let mut evidence = fixture();
        evidence.losses.insert(LossCategory::Product, 2);
        let gates = capability_gates(&[evidence]);

        assert!(gates.iter().all(|gate| gate.passed));
    }

    #[test]
    fn external_only_assembly_is_inapplicable_to_part_design_gates() {
        let mut assembly = fixture();
        assembly.rederivation = VerificationStatus::Inapplicable;
        assembly.losses.insert(LossCategory::DesignIntent, 2);

        let gates = capability_gates(&[assembly]);

        assert!(gates.iter().all(|gate| gate.passed));
        assert_eq!(
            gates[3].assertions[0].observed, "1/1 fixtures",
            "assembly topology remains in the cumulative profile"
        );
        assert_eq!(gates[4].assertions[0].observed, "0/0 applicable fixtures");
        assert_eq!(gates[6].assertions[1].observed, "0/0 applicable fixtures");
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
