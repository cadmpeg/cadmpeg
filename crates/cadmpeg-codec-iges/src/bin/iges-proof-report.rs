// SPDX-License-Identifier: Apache-2.0
//! Generate or verify the cumulative IGES Envelope-A proof report.

use cadmpeg_codec_iges::IgesCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
struct Matrix {
    schema_version: u64,
    envelope: String,
    representation: String,
    specification_version: String,
    approval: String,
    entity: Vec<MatrixEntity>,
}

#[derive(Deserialize)]
struct MatrixEntity {
    r#type: i64,
    name: String,
    forms: toml::Value,
    domain: String,
    decoder: String,
    destination: String,
    fixture_classes: Vec<String>,
    assertions: Vec<String>,
}

#[derive(Deserialize)]
struct OriginalEvidence {
    schema_version: u64,
    fixture_class: Vec<OriginalFixtureClass>,
}

#[derive(Deserialize)]
struct OriginalFixtureClass {
    name: String,
    tests: Vec<String>,
}

#[derive(Deserialize)]
struct PublicEvidence {
    schema_version: u64,
    fixtures: Vec<PublicFixture>,
}

#[derive(Deserialize)]
struct Approvals {
    schema_version: u64,
    envelope_matrix: String,
    ladder_decisions: String,
    byte_ledger: String,
}

#[derive(Deserialize)]
struct LadderDecisionFile {
    schema_version: u64,
    decision: Vec<LadderDecision>,
}

#[derive(Deserialize)]
struct LadderGateFile {
    schema_version: u64,
    gate: Vec<LadderGate>,
}

#[derive(Deserialize)]
struct LadderGate {
    level: String,
    decisions: Vec<String>,
    fixture_classes: Vec<String>,
    assertions: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
struct LadderDecision {
    gate: String,
    disposition: String,
    requirement: String,
}

#[derive(Deserialize)]
struct PublicFixture {
    filename: String,
    fixture_classes: Vec<String>,
    assertions: Vec<String>,
    tests: Vec<String>,
    inspect_sha256: String,
    ir_sha256: String,
    report_sha256: String,
}

#[derive(Deserialize)]
struct CorpusManifest {
    file: Vec<CorpusFile>,
}

#[derive(Deserialize)]
struct CorpusFile {
    filename: String,
    format: String,
    authoring_app: String,
    authoring_app_version: String,
    source_url: String,
    acquisition_date: String,
    license: String,
    sha256: String,
}

#[derive(Serialize)]
struct Report {
    schema_version: u64,
    envelope: String,
    representation: String,
    specification_version: String,
    approvals: ApprovalReport,
    ladder_decisions: Vec<LadderDecision>,
    summary: Summary,
    ladder_gates: Vec<ReportGate>,
    rows: Vec<ReportRow>,
    evidence_errors: Vec<String>,
}

#[derive(Serialize)]
struct ApprovalReport {
    envelope_matrix: String,
    ladder_decisions: String,
    byte_ledger: String,
}

#[derive(Serialize)]
struct Summary {
    matrix_rows: usize,
    original_fixture_classes: usize,
    public_fixtures: usize,
    structurally_complete_rows: usize,
    original_complete_rows: usize,
    public_complete_rows: usize,
    complete_rows: usize,
    structurally_complete_gates: usize,
    original_complete_gates: usize,
    public_complete_gates: usize,
    complete_gates: usize,
    release_ready: bool,
}

#[derive(Serialize)]
struct ReportGate {
    level: String,
    decisions: Vec<String>,
    fixture_classes: Vec<String>,
    assertions: Vec<String>,
    original_tests: Vec<String>,
    public_fixtures: Vec<String>,
    missing: Vec<String>,
}

#[derive(Serialize)]
struct ReportRow {
    entity_type: i64,
    name: String,
    forms: toml::Value,
    domain: String,
    decoder: String,
    destination: String,
    fixture_classes: Vec<String>,
    assertions: Vec<String>,
    original_tests: Vec<String>,
    public_fixtures: Vec<String>,
    missing: Vec<String>,
}

struct PublicOutputDigests {
    inspect: String,
    ir: String,
    report: String,
}

fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let source =
        fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    toml::from_str(&source).map_err(|error| format!("{}: {error}", path.display()))
}

fn decoder_exists(root: &Path, decoder: &str) -> bool {
    let components = decoder.split("::").collect::<Vec<_>>();
    let ["entities", module, function] = components.as_slice() else {
        return false;
    };
    let path = root
        .join("crates/cadmpeg-codec-iges/src/entities")
        .join(format!("{module}.rs"));
    fs::read_to_string(path)
        .ok()
        .is_some_and(|source| source.contains(&format!("fn {function}(")))
}

fn public_output_digests(bytes: &[u8], filename: &str) -> Result<PublicOutputDigests, String> {
    let inspect = IgesCodec
        .inspect(&mut Cursor::new(bytes))
        .map_err(|error| format!("public fixture {filename} inspect: {error}"))?;
    let first = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .map_err(|error| format!("public fixture {filename} decode: {error}"))?;
    let second = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .map_err(|error| format!("public fixture {filename} repeated decode: {error}"))?;
    let inspect_json = serde_json::to_vec(&inspect)
        .map_err(|error| format!("public fixture {filename} inspect serialization: {error}"))?;
    let first_ir = first.ir.to_canonical_json().map_err(|error| {
        format!("public fixture {filename} canonical IR serialization: {error}")
    })?;
    let second_ir = second.ir.to_canonical_json().map_err(|error| {
        format!("public fixture {filename} repeated canonical IR serialization: {error}")
    })?;
    let first_report = serde_json::to_vec(&first.report)
        .map_err(|error| format!("public fixture {filename} report serialization: {error}"))?;
    let second_report = serde_json::to_vec(&second.report).map_err(|error| {
        format!("public fixture {filename} repeated report serialization: {error}")
    })?;
    if first_ir != second_ir || first_report != second_report {
        return Err(format!(
            "public fixture {filename} decode is not deterministic"
        ));
    }
    Ok(PublicOutputDigests {
        inspect: cadmpeg_ir::hash::sha256_hex(&inspect_json),
        ir: cadmpeg_ir::hash::sha256_hex(first_ir.as_bytes()),
        report: cadmpeg_ir::hash::sha256_hex(&first_report),
    })
}

fn valid_public_fixture(
    root: &Path,
    fixture: &PublicFixture,
    manifest: &BTreeMap<String, CorpusFile>,
) -> Result<(), String> {
    if fixture.filename.is_empty()
        || fixture.fixture_classes.is_empty()
        || fixture.assertions.is_empty()
        || fixture.tests.is_empty()
        || fixture.inspect_sha256.len() != 64
        || fixture.ir_sha256.len() != 64
        || fixture.report_sha256.len() != 64
    {
        return Err(format!(
            "public fixture evidence {:?} has incomplete proof fields",
            fixture.filename
        ));
    }
    let record = manifest.get(&fixture.filename).ok_or_else(|| {
        format!(
            "public fixture {} has no corpus manifest record",
            fixture.filename
        )
    })?;
    if record.format != "iges"
        || record.authoring_app.is_empty()
        || record.authoring_app_version.is_empty()
        || record.source_url.is_empty()
        || record.acquisition_date.is_empty()
        || record.license != "CC0-1.0"
    {
        return Err(format!(
            "public fixture {} has incomplete or invalid corpus metadata",
            fixture.filename
        ));
    }
    let path = root.join("corpus").join(&fixture.filename);
    let bytes = fs::read(&path).map_err(|error| {
        format!(
            "public fixture {} at {}: {error}",
            fixture.filename,
            path.display()
        )
    })?;
    let digest = cadmpeg_ir::hash::sha256_hex(&bytes);
    if !digest.eq_ignore_ascii_case(&record.sha256) {
        return Err(format!(
            "public fixture {} digest is {}, declared {}",
            fixture.filename, digest, record.sha256
        ));
    }
    let outputs = public_output_digests(&bytes, &fixture.filename)?;
    for (kind, actual, expected) in [
        ("inspect", outputs.inspect, &fixture.inspect_sha256),
        ("canonical IR", outputs.ir, &fixture.ir_sha256),
        ("report", outputs.report, &fixture.report_sha256),
    ] {
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!(
                "public fixture {} {kind} digest is {actual}, declared {expected}",
                fixture.filename
            ));
        }
    }
    Ok(())
}

fn build_report(root: &Path) -> Result<Report, String> {
    let matrix: Matrix = read_toml(&root.join("corpus/iges-envelope-a.toml"))?;
    let original: OriginalEvidence = read_toml(&root.join("corpus/iges-original-evidence.toml"))?;
    let public: PublicEvidence = read_toml(&root.join("corpus/iges-public-evidence.toml"))?;
    let approvals: Approvals = read_toml(&root.join("corpus/iges-approvals.toml"))?;
    let decisions: LadderDecisionFile = read_toml(&root.join("corpus/iges-ladder-decisions.toml"))?;
    let gates: LadderGateFile = read_toml(&root.join("corpus/iges-ladder-gates.toml"))?;
    if matrix.schema_version != 1
        || original.schema_version != 1
        || public.schema_version != 1
        || approvals.schema_version != 1
        || decisions.schema_version != 1
        || gates.schema_version != 1
    {
        return Err("IGES proof inputs require schema_version = 1".into());
    }

    let test_source = fs::read_to_string(root.join("crates/cadmpeg-codec-iges/src/tests.rs"))
        .map_err(|error| format!("IGES test source: {error}"))?;
    let mut evidence_errors = Vec::new();
    if approvals.envelope_matrix != matrix.approval {
        evidence_errors.push(format!(
            "matrix approval {} differs from approval record {}",
            matrix.approval, approvals.envelope_matrix
        ));
    }
    let required_decisions = ["L0", "L3", "L4", "L6", "L7-mates"];
    let decision_gates = decisions
        .decision
        .iter()
        .map(|decision| decision.gate.as_str())
        .collect::<BTreeSet<_>>();
    for gate in required_decisions {
        if !decision_gates.contains(gate) {
            evidence_errors.push(format!("missing ladder decision {gate}"));
        }
    }
    if decision_gates.len() != decisions.decision.len() {
        evidence_errors.push("duplicate ladder decision gate".into());
    }
    for decision in &decisions.decision {
        if decision.disposition.is_empty() || decision.requirement.is_empty() {
            evidence_errors.push(format!("ladder decision {} is incomplete", decision.gate));
        }
    }
    let mut originals = BTreeMap::<String, Vec<String>>::new();
    for class in original.fixture_class {
        if class.name.is_empty() || class.tests.is_empty() {
            evidence_errors.push(format!(
                "original fixture class {:?} has no tests",
                class.name
            ));
            continue;
        }
        if originals.contains_key(&class.name) {
            evidence_errors.push(format!("duplicate original fixture class {}", class.name));
            continue;
        }
        for test in &class.tests {
            if !test_source.contains(&format!("fn {test}()")) {
                evidence_errors.push(format!(
                    "original fixture class {} names missing test {}",
                    class.name, test
                ));
            }
        }
        originals.insert(class.name, class.tests);
    }

    let manifest_path = root.join("corpus/manifest.toml");
    let manifest = if manifest_path.exists() {
        read_toml::<CorpusManifest>(&manifest_path)?
            .file
            .into_iter()
            .map(|file| (file.filename.clone(), file))
            .collect::<BTreeMap<_, _>>()
    } else {
        BTreeMap::new()
    };
    let mut valid_public = Vec::new();
    let mut public_ids = BTreeSet::new();
    for fixture in public.fixtures {
        if !public_ids.insert(fixture.filename.clone()) {
            evidence_errors.push(format!(
                "duplicate public fixture evidence {}",
                fixture.filename
            ));
            continue;
        }
        match valid_public_fixture(root, &fixture, &manifest) {
            Ok(()) => valid_public.push(fixture),
            Err(error) => evidence_errors.push(error),
        }
    }

    let required_gate_levels = (0..=8).map(|level| format!("L{level}")).collect::<Vec<_>>();
    let gate_levels = gates
        .gate
        .iter()
        .map(|gate| gate.level.as_str())
        .collect::<BTreeSet<_>>();
    for level in &required_gate_levels {
        if !gate_levels.contains(level.as_str()) {
            evidence_errors.push(format!("missing ladder gate {level}"));
        }
    }
    for level in &gate_levels {
        if !required_gate_levels
            .iter()
            .any(|required| required == level)
        {
            evidence_errors.push(format!("unexpected ladder gate {level}"));
        }
    }
    if gate_levels.len() != gates.gate.len() {
        evidence_errors.push("duplicate ladder gate level".into());
    }
    let known_assertions = matrix
        .entity
        .iter()
        .flat_map(|entity| entity.assertions.iter())
        .chain(gates.gate.iter().flat_map(|gate| gate.assertions.iter()))
        .collect::<BTreeSet<_>>();
    for fixture in &valid_public {
        let fixture_class_count = fixture
            .fixture_classes
            .iter()
            .collect::<BTreeSet<_>>()
            .len();
        if fixture_class_count != fixture.fixture_classes.len() {
            evidence_errors.push(format!(
                "public fixture {} repeats a fixture class",
                fixture.filename
            ));
        }
        for class in &fixture.fixture_classes {
            if !originals.contains_key(class) {
                evidence_errors.push(format!(
                    "public fixture {} names unknown fixture class {}",
                    fixture.filename, class
                ));
            }
        }
        let assertion_count = fixture.assertions.iter().collect::<BTreeSet<_>>().len();
        if assertion_count != fixture.assertions.len() {
            evidence_errors.push(format!(
                "public fixture {} repeats an assertion",
                fixture.filename
            ));
        }
        for assertion in &fixture.assertions {
            if !known_assertions.contains(assertion) {
                evidence_errors.push(format!(
                    "public fixture {} names unknown assertion {}",
                    fixture.filename, assertion
                ));
            }
        }
        let test_count = fixture.tests.iter().collect::<BTreeSet<_>>().len();
        if test_count != fixture.tests.len() {
            evidence_errors.push(format!(
                "public fixture {} repeats a test",
                fixture.filename
            ));
        }
        for test in &fixture.tests {
            if !test_source.contains(&format!("fn {test}()")) {
                evidence_errors.push(format!(
                    "public fixture {} names missing test {}",
                    fixture.filename, test
                ));
            }
        }
    }
    let mut ladder_gates = Vec::new();
    for gate in gates.gate {
        let mut missing = Vec::new();
        if gate.assertions.is_empty() {
            missing.push("assertion".into());
        }
        for decision in &gate.decisions {
            if !decision_gates.contains(decision.as_str()) {
                missing.push(format!("decision:{decision}"));
            }
        }
        if !gate.decisions.is_empty() && approvals.ladder_decisions != "approved" {
            missing.push("approval:ladder_decisions".into());
        }
        let mut original_tests = BTreeSet::new();
        let mut gate_public_ids = BTreeSet::new();
        for class in &gate.fixture_classes {
            match originals.get(class) {
                Some(tests) => original_tests.extend(tests.iter().cloned()),
                None => missing.push(format!("original_fixture_class:{class}")),
            }
            let fixtures = valid_public
                .iter()
                .filter(|fixture| fixture.fixture_classes.contains(class))
                .collect::<Vec<_>>();
            if fixtures.is_empty() {
                missing.push(format!("public_fixture_class:{class}"));
            } else {
                gate_public_ids
                    .extend(fixtures.into_iter().map(|fixture| fixture.filename.clone()));
            }
        }
        let relevant_public = valid_public
            .iter()
            .filter(|fixture| {
                fixture
                    .fixture_classes
                    .iter()
                    .any(|class| gate.fixture_classes.contains(class))
            })
            .collect::<Vec<_>>();
        for assertion in &gate.assertions {
            if !relevant_public
                .iter()
                .any(|fixture| fixture.assertions.contains(assertion))
                && !gate.fixture_classes.is_empty()
            {
                missing.push(format!("public_assertion:{assertion}"));
            }
        }
        ladder_gates.push(ReportGate {
            level: gate.level,
            decisions: gate.decisions,
            fixture_classes: gate.fixture_classes,
            assertions: gate.assertions,
            original_tests: original_tests.into_iter().collect(),
            public_fixtures: gate_public_ids.into_iter().collect(),
            missing,
        });
    }

    let mut rows = Vec::new();
    for entity in matrix.entity {
        let mut missing = Vec::new();
        if !decoder_exists(root, &entity.decoder) {
            missing.push("decoder".into());
        }
        if entity.destination.is_empty() {
            missing.push("destination".into());
        }
        if entity.assertions.is_empty() {
            missing.push("assertion".into());
        }
        let mut original_tests = BTreeSet::new();
        let mut public_ids = BTreeSet::new();
        for class in &entity.fixture_classes {
            match originals.get(class) {
                Some(tests) => original_tests.extend(tests.iter().cloned()),
                None => missing.push(format!("original_fixture_class:{class}")),
            }
            let fixtures = valid_public
                .iter()
                .filter(|fixture| fixture.fixture_classes.contains(class))
                .collect::<Vec<_>>();
            if fixtures.is_empty() {
                missing.push(format!("public_fixture_class:{class}"));
            } else {
                public_ids.extend(fixtures.into_iter().map(|fixture| fixture.filename.clone()));
            }
        }
        let relevant_public = valid_public
            .iter()
            .filter(|fixture| {
                fixture
                    .fixture_classes
                    .iter()
                    .any(|class| entity.fixture_classes.contains(class))
            })
            .collect::<Vec<_>>();
        for assertion in &entity.assertions {
            if !relevant_public
                .iter()
                .any(|fixture| fixture.assertions.contains(assertion))
            {
                missing.push(format!("public_assertion:{assertion}"));
            }
        }
        rows.push(ReportRow {
            entity_type: entity.r#type,
            name: entity.name,
            forms: entity.forms,
            domain: entity.domain,
            decoder: entity.decoder,
            destination: entity.destination,
            fixture_classes: entity.fixture_classes,
            assertions: entity.assertions,
            original_tests: original_tests.into_iter().collect(),
            public_fixtures: public_ids.into_iter().collect(),
            missing,
        });
    }
    let structurally_complete_rows = rows
        .iter()
        .filter(|row| {
            !row.missing
                .iter()
                .any(|missing| matches!(missing.as_str(), "decoder" | "destination" | "assertion"))
        })
        .count();
    let original_complete_rows = rows
        .iter()
        .filter(|row| {
            !row.missing
                .iter()
                .any(|missing| missing.starts_with("original_fixture_class:"))
        })
        .count();
    let public_complete_rows = rows
        .iter()
        .filter(|row| {
            !row.missing.iter().any(|missing| {
                missing.starts_with("public_fixture_class:")
                    || missing.starts_with("public_assertion:")
            })
        })
        .count();
    let complete_rows = rows.iter().filter(|row| row.missing.is_empty()).count();
    let structurally_complete_gates = ladder_gates
        .iter()
        .filter(|gate| {
            !gate
                .missing
                .iter()
                .any(|missing| missing == "assertion" || missing.starts_with("decision:"))
        })
        .count();
    let original_complete_gates = ladder_gates
        .iter()
        .filter(|gate| {
            !gate
                .missing
                .iter()
                .any(|missing| missing.starts_with("original_fixture_class:"))
        })
        .count();
    let public_complete_gates = ladder_gates
        .iter()
        .filter(|gate| {
            !gate.missing.iter().any(|missing| {
                missing.starts_with("public_fixture_class:")
                    || missing.starts_with("public_assertion:")
            })
        })
        .count();
    let complete_gates = ladder_gates
        .iter()
        .filter(|gate| gate.missing.is_empty())
        .count();
    let release_ready = approvals.envelope_matrix == "approved"
        && approvals.ladder_decisions == "approved"
        && approvals.byte_ledger == "approved"
        && evidence_errors.is_empty()
        && complete_rows == rows.len()
        && complete_gates == ladder_gates.len();
    Ok(Report {
        schema_version: 1,
        envelope: matrix.envelope,
        representation: matrix.representation,
        specification_version: matrix.specification_version,
        approvals: ApprovalReport {
            envelope_matrix: approvals.envelope_matrix,
            ladder_decisions: approvals.ladder_decisions,
            byte_ledger: approvals.byte_ledger,
        },
        ladder_decisions: decisions.decision,
        summary: Summary {
            matrix_rows: rows.len(),
            original_fixture_classes: originals.len(),
            public_fixtures: valid_public.len(),
            structurally_complete_rows,
            original_complete_rows,
            public_complete_rows,
            complete_rows,
            structurally_complete_gates,
            original_complete_gates,
            public_complete_gates,
            complete_gates,
            release_ready,
        },
        ladder_gates,
        rows,
        evidence_errors,
    })
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn serialized_report(root: &Path) -> Result<String, String> {
    Ok(serde_json::to_string_pretty(&build_report(root)?)
        .map_err(|error| format!("serialize IGES proof report: {error}"))?
        + "\n")
}

fn run() -> Result<(), String> {
    const USAGE: &str = "usage: iges-proof-report (--write|--check|--profile-public) <path>";
    let mut arguments = std::env::args().skip(1);
    let mode = arguments.next().ok_or_else(|| USAGE.to_string())?;
    let path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| USAGE.to_string())?;
    if arguments.next().is_some() {
        return Err(USAGE.into());
    }
    match mode.as_str() {
        "--write" => {
            let report = serialized_report(&workspace_root())?;
            fs::write(&path, report).map_err(|error| format!("{}: {error}", path.display()))
        }
        "--check" => {
            let report = serialized_report(&workspace_root())?;
            let current = fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            if current == report {
                Ok(())
            } else {
                Err(format!(
                    "{} is stale; regenerate it with --write",
                    path.display()
                ))
            }
        }
        "--profile-public" => {
            let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            let outputs = public_output_digests(&bytes, &path.display().to_string())?;
            println!("inspect_sha256 = {:?}", outputs.inspect);
            println!("ir_sha256 = {:?}", outputs.ir);
            println!("report_sha256 = {:?}", outputs.report);
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::{build_report, public_output_digests, serialized_report, workspace_root};

    fn card(data: &[u8], section: u8, sequence: u32) -> Vec<u8> {
        let mut result = vec![b' '; 80];
        result[..data.len()].copy_from_slice(data);
        result[72] = section;
        result[73..80].copy_from_slice(format!("{sequence:07}").as_bytes());
        result.push(b'\n');
        result
    }

    fn empty_fixed_ascii_document() -> Vec<u8> {
        let global = b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;";
        let mut bytes = card(b"original fixture", b'S', 1);
        let chunks = global.chunks(72).collect::<Vec<_>>();
        for (index, chunk) in chunks.iter().enumerate() {
            bytes.extend(card(chunk, b'G', u32::try_from(index + 1).unwrap()));
        }
        bytes.extend(card(
            format!("S0000001G{:07}D0000000P0000000", chunks.len()).as_bytes(),
            b'T',
            1,
        ));
        bytes
    }

    #[test]
    fn public_profile_hashes_deterministic_decode_outputs() {
        let digests = public_output_digests(&empty_fixed_ascii_document(), "empty.igs")
            .expect("fixture profiles");

        assert_eq!(digests.inspect.len(), 64);
        assert_eq!(digests.ir.len(), 64);
        assert_eq!(digests.report.len(), 64);
    }

    #[test]
    fn current_evidence_proves_original_but_not_public_coverage() {
        let report = build_report(&workspace_root()).expect("proof inputs are valid");

        assert_eq!(report.summary.matrix_rows, 81);
        assert_eq!(report.summary.structurally_complete_rows, 81);
        assert_eq!(report.summary.original_complete_rows, 81);
        assert_eq!(report.summary.public_complete_rows, 0);
        assert_eq!(report.summary.complete_rows, 0);
        assert_eq!(report.summary.structurally_complete_gates, 9);
        assert_eq!(report.summary.original_complete_gates, 9);
        assert_eq!(report.summary.public_complete_gates, 2);
        assert_eq!(report.summary.complete_gates, 0);
        assert!(!report.summary.release_ready);
        assert!(report.evidence_errors.is_empty());
    }

    #[test]
    fn committed_report_is_current() {
        let root = workspace_root();
        let committed = std::fs::read_to_string(root.join("corpus/iges-envelope-a-proof.json"))
            .expect("committed proof report exists");
        let generated = serialized_report(&root).expect("proof report generates");

        assert_eq!(committed, generated);
    }
}
