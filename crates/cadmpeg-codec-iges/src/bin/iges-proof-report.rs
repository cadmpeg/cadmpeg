// SPDX-License-Identifier: Apache-2.0
//! Generate or verify the cumulative IGES Envelope-A proof report.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
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
struct PublicFixture {
    filename: String,
    fixture_classes: Vec<String>,
    assertions: Vec<String>,
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
    approval: String,
    summary: Summary,
    rows: Vec<ReportRow>,
    evidence_errors: Vec<String>,
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
    release_ready: bool,
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

fn valid_public_fixture(
    root: &Path,
    fixture: &PublicFixture,
    manifest: &BTreeMap<String, CorpusFile>,
) -> Result<(), String> {
    if fixture.filename.is_empty()
        || fixture.fixture_classes.is_empty()
        || fixture.assertions.is_empty()
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
    Ok(())
}

fn build_report(root: &Path) -> Result<Report, String> {
    let matrix: Matrix = read_toml(&root.join("corpus/iges-envelope-a.toml"))?;
    let original: OriginalEvidence = read_toml(&root.join("corpus/iges-original-evidence.toml"))?;
    let public: PublicEvidence = read_toml(&root.join("corpus/iges-public-evidence.toml"))?;
    if matrix.schema_version != 1 || original.schema_version != 1 || public.schema_version != 1 {
        return Err("IGES proof inputs require schema_version = 1".into());
    }

    let test_source = fs::read_to_string(root.join("crates/cadmpeg-codec-iges/src/tests.rs"))
        .map_err(|error| format!("IGES test source: {error}"))?;
    let mut evidence_errors = Vec::new();
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
            !row.missing
                .iter()
                .any(|missing| missing.starts_with("public_fixture_class:"))
        })
        .count();
    let complete_rows = rows.iter().filter(|row| row.missing.is_empty()).count();
    let release_ready =
        matrix.approval == "approved" && evidence_errors.is_empty() && complete_rows == rows.len();
    Ok(Report {
        schema_version: 1,
        envelope: matrix.envelope,
        representation: matrix.representation,
        specification_version: matrix.specification_version,
        approval: matrix.approval,
        summary: Summary {
            matrix_rows: rows.len(),
            original_fixture_classes: originals.len(),
            public_fixtures: valid_public.len(),
            structurally_complete_rows,
            original_complete_rows,
            public_complete_rows,
            complete_rows,
            release_ready,
        },
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
    let report = serialized_report(&workspace_root())?;
    let mut arguments = std::env::args().skip(1);
    let mode = arguments
        .next()
        .ok_or_else(|| "usage: iges-proof-report (--write|--check) <path>".to_string())?;
    let path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "usage: iges-proof-report (--write|--check) <path>".to_string())?;
    if arguments.next().is_some() {
        return Err("usage: iges-proof-report (--write|--check) <path>".into());
    }
    match mode.as_str() {
        "--write" => {
            fs::write(&path, report).map_err(|error| format!("{}: {error}", path.display()))
        }
        "--check" => {
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
        _ => Err("usage: iges-proof-report (--write|--check) <path>".into()),
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
    use super::{build_report, serialized_report, workspace_root};

    #[test]
    fn current_evidence_proves_original_but_not_public_coverage() {
        let report = build_report(&workspace_root()).expect("proof inputs are valid");

        assert_eq!(report.summary.matrix_rows, 81);
        assert_eq!(report.summary.structurally_complete_rows, 81);
        assert_eq!(report.summary.original_complete_rows, 81);
        assert_eq!(report.summary.public_complete_rows, 0);
        assert_eq!(report.summary.complete_rows, 0);
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
