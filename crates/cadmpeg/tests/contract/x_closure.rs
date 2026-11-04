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
    fixed_paths: std::collections::BTreeSet<String>,
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
/// The carried set is observed from the head build decoding STEP's own
/// goldens; scan and table variants are not attributed yet (only
/// `scan_step.json` is checked in). STEP stores nothing in reduced form, so
/// there are no reduced probes.
///
/// A debug build runs the §9.4 fixed sample: the first fixture of each format
/// in path order. `cargo test --release --test contract` runs all.
struct StepObserved {
    carried: cadmpeg_test_support::carried::Carried,
    fixed_paths: std::collections::BTreeSet<String>,
    known_losses: std::collections::BTreeSet<String>,
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
    let fixed_paths = single_value_paths(&documents);
    let carried = observe_carried(&documents);
    StepObserved {
        carried,
        fixed_paths,
        known_losses,
    }
}

fn run_step_other_format(format: &str, observed: &StepObserved) -> Vec<String> {
    let encoder = cadmpeg_registry::build_encoder(Format::Step);
    let source = codec(format);
    let step = codec("step");
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
    fixed_paths: std::collections::BTreeSet<String>,
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
/// variants are not attributed. FreeCAD declares no reduced fields yet
/// (#238), so every path compares exactly until its `REDUCED_FIELDS` rows
/// land.
///
/// A debug build runs the §9.4 fixed sample: the first fixture of each format
/// in path order. `cargo test --release --test contract` runs all.
struct ObservedFcstd {
    carried: cadmpeg_test_support::carried::Carried,
    fixed_paths: std::collections::BTreeSet<String>,
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
    // FreeCAD declares no reduced fields yet (#238); every path compares
    // exactly until its REDUCED_FIELDS rows land.
    let reduced = Vec::new();
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
