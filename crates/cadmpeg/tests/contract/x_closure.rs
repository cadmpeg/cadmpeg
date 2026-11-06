// SPDX-License-Identifier: Apache-2.0
//! W-closure-cross (§5.2, #232) for the Rhino writer.
//!
//! The carried set is observed from the head build decoding Rhino's own
//! goldens ([`observe_carried`]); the per-crate constructor scan
//! (`scan_rhino.json`) and the observation/scan/table composition helper do
//! not exist yet (only `scan_step.json` is checked in), so scan and table
//! variants are not attributed. Reduced probes use the `REDUCED_FIELDS`
//! spellings; the vertices row is f64 at the cross default (archive 80) and
//! needs no probe there.
//!
//! A debug build runs the §9.4 fixed sample: the first fixture of each format
//! in path order. `cargo test --release --test contract` runs all.

use std::io::Cursor;
use std::path::PathBuf;

use cadmpeg_ir::codec::write::{Encoder, ReducedStorage};
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_registry::Format;
use cadmpeg_test_support::carried::observe_carried;
use cadmpeg_test_support::cross_closure::{
    cross_findings, single_value_paths, CrossConfig, ReducedProbe,
};
use cadmpeg_test_support::writer_closure::Redecoded;

use super::conformance::fixtures;

fn codec(format: &str) -> &'static dyn Codec {
    let catalog = Box::leak(Box::new(cadmpeg_registry::InputCatalog::with_builtins()));
    catalog
        .descriptors()
        .find(|descriptor| descriptor.format_id().as_str() == format)
        .and_then(cadmpeg_registry::InputDescriptor::codec)
        .unwrap_or_else(|| panic!("{format} has no codec in this build"))
}

/// Every fixture of `format`, or in a debug build its first fixture.
fn sampled(format: &str) -> Vec<PathBuf> {
    let mut paths = fixtures(format);
    paths.sort();
    if cfg!(debug_assertions) {
        paths.truncate(1);
    }
    paths
}

fn decode(codec: &dyn Codec, encoder: &dyn Encoder, bytes: &[u8]) -> Result<DecodeResult, String> {
    let options = DecodeOptions {
        depth: encoder.request(),
        ..DecodeOptions::default()
    };
    codec
        .decode(&mut Cursor::new(bytes), &options)
        .map_err(|error| error.to_string())
}

fn loss_codes(result: &DecodeResult) -> std::collections::BTreeSet<String> {
    result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.to_string())
        .collect()
}

/// Observation inputs for the Rhino cross comparison, from Rhino's own goldens.
struct Observed {
    carried: cadmpeg_test_support::carried::Carried,
    fixed_paths: std::collections::BTreeMap<String, serde_json::Value>,
    known_losses: std::collections::BTreeSet<String>,
    reduced: Vec<ReducedProbe>,
}

fn observe() -> Observed {
    let encoder = cadmpeg_registry::build_encoder(Format::Rhino);
    let source = codec("rhino");
    let mut documents = Vec::new();
    let mut known_losses = std::collections::BTreeSet::new();
    for path in sampled("rhino") {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        known_losses.extend(loss_codes(&first));
        documents.push(serde_json::to_value(first.ir()).expect("CADIR JSON"));
    }
    let fixed_paths = single_value_paths(&documents);
    let carried = observe_carried(&documents);
    let reduced = [
        (
            "model.tessellations[].vertex_normals[]",
            ReducedStorage::F32,
        ),
        ("model.bodies[].color", ReducedStorage::U8Channel),
        ("model.appearances[].base_color", ReducedStorage::U8Channel),
    ]
    .into_iter()
    .map(|(path, storage)| ReducedProbe {
        path,
        storage,
        normalize: None,
    })
    .collect();
    Observed {
        carried,
        fixed_paths,
        known_losses,
        reduced,
    }
}

fn run_other_format(format: &str, observed: &Observed) -> Vec<String> {
    let encoder = cadmpeg_registry::build_encoder(Format::Rhino);
    let source = codec(format);
    let rhino = codec("rhino");
    let mut findings = Vec::new();
    for path in sampled(format) {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            let result = decode(rhino, encoder.as_ref(), written)?;
            Ok(Redecoded {
                members: Vec::new(),
                ir: serde_json::to_value(result.ir()).map_err(|error| error.to_string())?,
                loss_codes: loss_codes(&result),
            })
        };
        let config = CrossConfig {
            carried: &observed.carried,
            fixed_paths: &observed.fixed_paths,
            known_losses: &observed.known_losses,
            reduced: &observed.reduced,
            members: &[],
        };
        findings.extend(
            cross_findings(&first, encoder.as_ref(), &redecode, &config)
                .into_iter()
                .map(|finding| format!("{}: {finding}", path.display())),
        );
    }
    findings
}

fn assert_none(findings: &[String]) {
    assert!(
        findings.is_empty(),
        "{} finding(s):\n{}",
        findings.len(),
        findings.join("\n")
    );
}

#[test]
fn rhino_cross_closure_carries_other_formats() {
    let observed = observe();
    let mut findings = Vec::new();
    for format in ["step", "iges", "fcstd", "f3d", "sldprt", "catia", "creo"] {
        findings.extend(run_other_format(format, &observed));
    }
    assert_none(&findings);
}

/// W-closure-cross (§5.2, #232) for the STEP writer.
///
/// The carried set combines STEP's decoded goldens with its constructor
/// scan, which includes variants from the constructor table. STEP stores
/// nothing in reduced form, so there are no reduced probes.
///
/// A debug build runs the §9.4 fixed sample: the first fixture of each format
/// in path order. `cargo test --release --test contract` runs all.
struct StepObserved {
    carried: cadmpeg_test_support::carried::Carried,
    fixed_paths: std::collections::BTreeMap<String, serde_json::Value>,
    known_losses: std::collections::BTreeSet<String>,
}

fn step_seed_paths() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/cadmpeg-fuzz/seeds");
    let mut pending: Vec<_> = std::fs::read_dir(&root)
        .expect("STEP fuzz seed directory")
        .map(|entry| entry.expect("seed entry").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("step"))
        })
        .collect();
    let mut paths = Vec::new();
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            pending.extend(
                std::fs::read_dir(&path)
                    .expect("seed subdirectory")
                    .map(|entry| entry.expect("seed entry").path()),
            );
        } else {
            paths.push(path);
        }
    }
    paths.sort();
    assert!(!paths.is_empty(), "STEP fuzz seeds are missing");
    paths
}

fn step_base_documents() -> Vec<serde_json::Value> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let base = cadmpeg_test_support::base::resolve_base();
    let output = std::process::Command::new("git")
        .args([
            "ls-tree",
            "-r",
            "--name-only",
            &base,
            "--",
            "crates/cadmpeg-codec-step/tests/golden/decode",
        ])
        .current_dir(&root)
        .output()
        .expect("list base STEP goldens");
    assert!(output.status.success(), "cannot list base STEP goldens");
    let names = String::from_utf8(output.stdout).expect("base golden paths are UTF-8");
    let documents: Vec<_> = names
        .lines()
        .filter(|path| path.ends_with(".json"))
        .filter_map(|path| {
            let text = cadmpeg_test_support::base::base_text_at(&root, &base, path)
                .expect("listed base golden exists");
            cadmpeg_test_support::base::decode_golden_from_text(path, &text).map(|golden| golden.ir)
        })
        .collect();
    assert!(
        !documents.is_empty(),
        "base STEP decode goldens are missing"
    );
    documents
}

fn step_observed_sets(
    fixtures: &[serde_json::Value],
    seeds: &[serde_json::Value],
    base: &[serde_json::Value],
) -> (
    cadmpeg_test_support::carried::Carried,
    std::collections::BTreeMap<String, serde_json::Value>,
) {
    let fixed = single_value_paths(fixtures);
    let mut carried = observe_carried(fixtures);
    let seed_carried = observe_carried(seeds);
    carried.arenas.extend(seed_carried.arenas);
    carried.variants.extend(seed_carried.variants);
    carried.paths.extend(seed_carried.paths);
    let base_carried = observe_carried(base);
    carried.arenas.extend(base_carried.arenas);
    carried.variants.extend(base_carried.variants);
    (carried, fixed)
}

fn step_observe() -> StepObserved {
    let encoder = cadmpeg_registry::build_encoder(Format::Step);
    let source = codec("step");
    let mut documents = Vec::new();
    let mut known_losses = std::collections::BTreeSet::new();
    for path in sampled("step") {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        known_losses.extend(loss_codes(&first));
        documents.push(serde_json::to_value(first.ir()).expect("CADIR JSON"));
    }
    assert!(!documents.is_empty(), "no STEP golden fixture decoded");
    let mut seed_documents = Vec::new();
    for path in step_seed_paths() {
        let bytes = std::fs::read(&path).expect("STEP seed bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        seed_documents.push(serde_json::to_value(first.ir()).expect("CADIR JSON"));
    }
    assert!(!seed_documents.is_empty(), "no STEP fuzz seed decoded");
    documents.extend(
        cadmpeg_test_support::data_checks::decode_goldens("cadmpeg-codec-step")
            .into_iter()
            .map(|golden| golden.ir),
    );
    let (mut carried, fixed_paths) =
        step_observed_sets(&documents, &seed_documents, &step_base_documents());
    let scan = cadmpeg_test_support::carried::crate_scan(include_str!(
        "../../../cadmpeg-test-support/src/data/scan_step.json"
    ));
    carried.arenas.extend(scan.arenas);
    carried.variants.extend(scan.variants);
    use cadmpeg_test_support::carried::EdgeCarrierVariant;
    carried.edge_carriers = Some(std::collections::BTreeSet::from([
        EdgeCarrierVariant::Curve,
        EdgeCarrierVariant::Bounded,
    ]));
    StepObserved {
        carried,
        fixed_paths,
        known_losses,
    }
}

#[test]
fn step_observation_combines_fixtures_seeds_and_base() {
    let fixture = serde_json::json!({"model":{"points":[{"id":"fixture", "visible":true}]}});
    let seed = serde_json::json!({"model":{"surfaces":[{"id":"seed", "seed_field":1}]}});
    let base = serde_json::json!({"model":{"curves":[{"id":"base", "base_field":1}]}});
    let (carried, fixed) = step_observed_sets(&[fixture], &[seed], &[base]);
    assert!(carried.arenas.contains("points"));
    assert!(carried.arenas.contains("surfaces"));
    assert!(carried.arenas.contains("curves"));
    assert!(carried.paths.contains("model.surfaces[].seed_field"));
    assert!(!carried.paths.contains("model.curves[].base_field"));
    assert_eq!(
        fixed.get("model.points[].visible"),
        Some(&serde_json::json!(true))
    );
}

#[test]
fn step_observation_reads_fixture_seed_and_base_inputs() {
    let observed = step_observe();
    assert!(observed.carried.arenas.contains("surfaces"));
    assert!(!observed.carried.paths.is_empty());
}

fn run_step_other_format(format: &str, observed: &StepObserved) -> Vec<String> {
    let encoder = cadmpeg_registry::build_encoder(Format::Step);
    let source = codec(format);
    let step = codec("step");
    let reduced: Vec<_> = encoder
        .reduced_fields()
        .iter()
        .map(|row| cadmpeg_test_support::cross_closure::ReducedProbe {
            path: row.path,
            storage: row.storage,
            normalize: match row.storage {
                cadmpeg_ir::codec::write::ReducedStorage::Normalized {
                    loss: "step/geometry.analytic-surface-normalized",
                } => Some(cadmpeg_test_support::reduced::step_surface_chart),
                cadmpeg_ir::codec::write::ReducedStorage::Normalized {
                    loss: "step/attribute.timestamp-normalized",
                } => Some(cadmpeg_test_support::reduced::step_timestamp_attribute),
                cadmpeg_ir::codec::write::ReducedStorage::Normalized { loss } => {
                    panic!("unknown STEP normalization {loss}")
                }
                _ => None,
            },
        })
        .collect();
    let mut findings = Vec::new();
    for path in sampled(format) {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            let result = decode(step, encoder.as_ref(), written)?;
            Ok(Redecoded {
                members: Vec::new(),
                ir: serde_json::to_value(result.ir()).map_err(|error| error.to_string())?,
                loss_codes: loss_codes(&result),
            })
        };
        findings.extend(
            cross_findings(
                &first,
                encoder.as_ref(),
                &redecode,
                &CrossConfig {
                    carried: &observed.carried,
                    fixed_paths: &observed.fixed_paths,
                    known_losses: &observed.known_losses,
                    reduced: &reduced,
                    members: &[],
                },
            )
            .into_iter()
            .map(|finding| format!("{}: {finding}", path.display())),
        );
    }
    findings
}

#[test]
fn step_cross_closure_carries_other_formats() {
    let observed = step_observe();
    let mut findings = Vec::new();
    for format in ["iges", "rhino", "fcstd", "f3d", "sldprt", "catia", "creo"] {
        findings.extend(run_step_other_format(format, &observed));
    }
    assert_none(&findings);
}

/// Observation inputs for the SolidWorks cross comparison, from SolidWorks'
/// own goldens (#237, §5.2).
///
/// The carried set is observed from the head build decoding SolidWorks' own
/// goldens ([`observe_carried`]); the per-crate constructor scan
/// (`scan_sldprt.json`) and the observation/scan/table composition helper do
/// not exist yet (only `scan_step.json` is checked in), so scan and table
/// variants are not attributed. SolidWorks' `REDUCED_FIELDS` rows compare
/// with their storage applied.
///
/// A debug build runs the §9.4 fixed sample: the first fixture of each format
/// in path order. `cargo test --release --test contract` runs all.
struct ObservedSldprt {
    carried: cadmpeg_test_support::carried::Carried,
    fixed_paths: std::collections::BTreeMap<String, serde_json::Value>,
    known_losses: std::collections::BTreeSet<String>,
    reduced: Vec<ReducedProbe>,
}

fn observe_sldprt() -> ObservedSldprt {
    let encoder = cadmpeg_registry::build_encoder(Format::Sldprt);
    let source = codec("sldprt");
    let mut documents = Vec::new();
    let mut known_losses = std::collections::BTreeSet::new();
    for path in sampled("sldprt") {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        known_losses.extend(loss_codes(&first));
        documents.push(serde_json::to_value(first.ir()).expect("CADIR JSON"));
    }
    let fixed_paths = single_value_paths(&documents);
    let carried = observe_carried(&documents);
    // SolidWorks stores tessellation scalars as f32 and appearance base
    // colours as 8-bit channels (#237 REDUCED_FIELDS); the comparison applies
    // that storage before pairing.
    let reduced = [
        ("model.tessellations[].vertices[]", ReducedStorage::F32),
        (
            "model.tessellations[].vertex_normals[]",
            ReducedStorage::F32,
        ),
        ("model.appearances[].base_color", ReducedStorage::U8Channel),
    ]
    .into_iter()
    .map(|(path, storage)| ReducedProbe {
        path,
        storage,
        normalize: None,
    })
    .collect();
    ObservedSldprt {
        carried,
        fixed_paths,
        known_losses,
        reduced,
    }
}

fn run_sldprt_other_format(format: &str, observed: &ObservedSldprt) -> Vec<String> {
    let encoder = cadmpeg_registry::build_encoder(Format::Sldprt);
    let source = codec(format);
    let sldprt = codec("sldprt");
    let mut findings = Vec::new();
    for path in sampled(format) {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            let result = decode(sldprt, encoder.as_ref(), written)?;
            Ok(Redecoded {
                members: Vec::new(),
                ir: serde_json::to_value(result.ir()).map_err(|error| error.to_string())?,
                loss_codes: loss_codes(&result),
            })
        };
        findings.extend(
            cross_findings(
                &first,
                encoder.as_ref(),
                &redecode,
                &CrossConfig {
                    carried: &observed.carried,
                    fixed_paths: &observed.fixed_paths,
                    known_losses: &observed.known_losses,
                    reduced: &observed.reduced,
                    members: &[],
                },
            )
            .into_iter()
            .map(|finding| format!("{}: {finding}", path.display())),
        );
    }
    findings
}

#[test]
fn sldprt_cross_closure_carries_other_formats() {
    let observed = observe_sldprt();
    let mut findings = Vec::new();
    for format in ["step", "iges", "rhino", "fcstd", "f3d", "catia", "creo"] {
        findings.extend(run_sldprt_other_format(format, &observed));
    }
    assert_none(&findings);
}
/// Observation inputs for the FreeCAD cross comparison, from FreeCAD's own
/// goldens (#238 §5.2).
///
/// The carried set is observed from the head build decoding FreeCAD's own
/// goldens ([`observe_carried`]); the per-crate constructor scan
/// (`scan_fcstd.json`) and the observation/scan/table composition helper do
/// not exist yet (only `scan_step.json` is checked in), so scan and table
/// variants are not attributed.
///
/// A debug build runs the §9.4 fixed sample: the first fixture of each format
/// in path order. `cargo test --release --test contract` runs all.
struct ObservedFcstd {
    carried: cadmpeg_test_support::carried::Carried,
    fixed_paths: std::collections::BTreeMap<String, serde_json::Value>,
    known_losses: std::collections::BTreeSet<String>,
    reduced: Vec<ReducedProbe>,
}

fn observe_fcstd() -> ObservedFcstd {
    let encoder = cadmpeg_registry::build_encoder(Format::Fcstd);
    let source = codec("fcstd");
    let mut documents = Vec::new();
    let mut known_losses = std::collections::BTreeSet::new();
    for path in sampled("fcstd") {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        known_losses.extend(loss_codes(&first));
        documents.push(serde_json::to_value(first.ir()).expect("CADIR JSON"));
    }
    let fixed_paths = single_value_paths(&documents);
    let carried = observe_carried(&documents);
    let reduced = encoder
        .reduced_fields()
        .iter()
        .map(|row| ReducedProbe {
            path: row.path,
            storage: row.storage,
            normalize: None,
        })
        .collect();
    ObservedFcstd {
        carried,
        fixed_paths,
        known_losses,
        reduced,
    }
}

fn run_fcstd_other_format(format: &str, observed: &ObservedFcstd) -> Vec<String> {
    let encoder = cadmpeg_registry::build_encoder(Format::Fcstd);
    let source = codec(format);
    let fcstd = codec("fcstd");
    let mut findings = Vec::new();
    for path in sampled(format) {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            let result = decode(fcstd, encoder.as_ref(), written)?;
            Ok(Redecoded {
                members: Vec::new(),
                ir: serde_json::to_value(result.ir()).map_err(|error| error.to_string())?,
                loss_codes: loss_codes(&result),
            })
        };
        findings.extend(
            cross_findings(
                &first,
                encoder.as_ref(),
                &redecode,
                &CrossConfig {
                    carried: &observed.carried,
                    fixed_paths: &observed.fixed_paths,
                    known_losses: &observed.known_losses,
                    reduced: &observed.reduced,
                    members: &[],
                },
            )
            .into_iter()
            .map(|finding| format!("{}: {finding}", path.display())),
        );
    }
    findings
}

#[test]
fn fcstd_cross_closure_carries_other_formats() {
    let observed = observe_fcstd();
    let mut findings = Vec::new();
    for format in ["step", "iges", "rhino", "f3d", "sldprt", "catia", "creo"] {
        findings.extend(run_fcstd_other_format(format, &observed));
    }
    assert_none(&findings);
}

struct ObservedF3d {
    carried: cadmpeg_test_support::carried::Carried,
    fixed_paths: std::collections::BTreeMap<String, serde_json::Value>,
    known_losses: std::collections::BTreeSet<String>,
    reduced: Vec<ReducedProbe>,
}

fn observe_f3d() -> ObservedF3d {
    let encoder = cadmpeg_registry::build_encoder(Format::F3d);
    let source = codec("f3d");
    let mut documents = Vec::new();
    let mut known_losses = std::collections::BTreeSet::new();
    for path in sampled("f3d") {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        known_losses.extend(loss_codes(&first));
        documents.push(serde_json::to_value(first.ir()).expect("CADIR JSON"));
    }
    let fixed_paths = single_value_paths(&documents);
    let carried = observe_carried(&documents);
    let reduced = encoder
        .reduced_fields()
        .iter()
        .map(|row| ReducedProbe {
            path: row.path,
            storage: row.storage,
            normalize: match row.path {
                "model.bodies[].color.a" | "model.faces[].color.a" => Some(|_| serde_json::Value::from(1.0)),
                _ => None,
            },
        })
        .collect();
    ObservedF3d {
        carried,
        fixed_paths,
        known_losses,
        reduced,
    }
}

fn run_f3d_other_format(format: &str, observed: &ObservedF3d) -> Vec<String> {
    let encoder = cadmpeg_registry::build_encoder(Format::F3d);
    let source = codec(format);
    let f3d = codec("f3d");
    let mut findings = Vec::new();
    for path in sampled(format) {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            let result = decode(f3d, encoder.as_ref(), written)?;
            Ok(Redecoded {
                members: Vec::new(),
                ir: serde_json::to_value(result.ir()).map_err(|error| error.to_string())?,
                loss_codes: loss_codes(&result),
            })
        };
        findings.extend(
            cross_findings(
                &first,
                encoder.as_ref(),
                &redecode,
                &CrossConfig {
                    carried: &observed.carried,
                    fixed_paths: &observed.fixed_paths,
                    known_losses: &observed.known_losses,
                    reduced: &observed.reduced,
                    members: &[],
                },
            )
            .into_iter()
            .map(|finding| format!("{}: {finding}", path.display())),
        );
    }
    findings
}

#[test]
fn f3d_cross_closure_carries_other_formats() {
    let observed = observe_f3d();
    let mut findings = Vec::new();
    for format in ["step", "iges", "rhino", "fcstd", "sldprt", "catia", "creo"] {
        findings.extend(run_f3d_other_format(format, &observed));
    }
    assert_none(&findings);
}

/// Observation and constructor-scan inputs for IGES (§5.2). The scan
/// includes variants supplied by CADIR constructors called by the decoder.
///
/// A debug build runs the §9.4 fixed sample: the first fixture of each format
/// in path order. `cargo test --release --test contract` runs all.
struct ObservedIges {
    carried: cadmpeg_test_support::carried::Carried,
    fixed_paths: std::collections::BTreeMap<String, serde_json::Value>,
    known_losses: std::collections::BTreeSet<String>,
}

fn observe_iges() -> ObservedIges {
    let encoder = cadmpeg_registry::build_encoder(Format::Iges);
    let source = codec("iges");
    let mut documents = Vec::new();
    let mut known_losses = std::collections::BTreeSet::new();
    for path in sampled("iges") {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        known_losses.extend(loss_codes(&first));
        documents.push(serde_json::to_value(first.ir()).expect("CADIR JSON"));
    }
    let fixed_paths = single_value_paths(&documents);
    let mut carried = observe_carried(&documents);
    let seeds = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../cadmpeg-fuzz/seeds");
    let mut seed_documents = Vec::new();
    for directory in std::fs::read_dir(seeds).expect("fuzz seed directory") {
        let directory = directory.expect("fuzz seed entry");
        if !directory.file_name().to_string_lossy().starts_with("iges")
            || !directory.file_type().expect("seed entry type").is_dir()
        {
            continue;
        }
        for seed in std::fs::read_dir(directory.path()).expect("IGES seed directory") {
            let seed = seed.expect("IGES seed entry");
            if !seed.file_type().expect("seed type").is_file() {
                continue;
            }
            let bytes = std::fs::read(seed.path()).expect("IGES seed bytes");
            if let Ok(decoded) = decode(source, encoder.as_ref(), &bytes) {
                seed_documents.push(serde_json::to_value(decoded.ir()).expect("CADIR JSON"));
            }
        }
    }
    let observed_seeds = observe_carried(&seed_documents);
    carried.arenas.extend(observed_seeds.arenas);
    carried.variants.extend(observed_seeds.variants);
    let scan_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../cadmpeg-test-support/src/data/scan_iges.json");
    let scan = cadmpeg_test_support::carried::crate_scan(
        &std::fs::read_to_string(scan_path).expect("IGES constructor scan"),
    );
    carried.arenas.extend(scan.arenas);
    carried.variants.extend(scan.variants);
    ObservedIges {
        carried,
        fixed_paths,
        known_losses,
    }
}

fn run_iges_other_format(format: &str, observed: &ObservedIges) -> Vec<String> {
    let encoder = cadmpeg_registry::build_encoder(Format::Iges);
    let source = codec(format);
    let iges = codec("iges");
    let mut findings = Vec::new();
    for path in sampled(format) {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            let result = decode(iges, encoder.as_ref(), written)?;
            Ok(Redecoded {
                members: Vec::new(),
                ir: serde_json::to_value(result.ir()).map_err(|error| error.to_string())?,
                loss_codes: loss_codes(&result),
            })
        };
        findings.extend(
            cross_findings(
                &first,
                encoder.as_ref(),
                &redecode,
                &CrossConfig {
                    carried: &observed.carried,
                    fixed_paths: &observed.fixed_paths,
                    known_losses: &observed.known_losses,
                    reduced: &[],
                    members: &[],
                },
            )
            .into_iter()
            .map(|finding| format!("{}: {finding}", path.display())),
        );
    }
    findings
}

#[test]
fn iges_cross_closure_carries_other_formats() {
    let observed = observe_iges();
    let mut findings = Vec::new();
    for format in ["step", "rhino", "fcstd", "f3d", "sldprt", "catia", "creo"] {
        findings.extend(run_iges_other_format(format, &observed));
    }
    assert_none(&findings);
}
