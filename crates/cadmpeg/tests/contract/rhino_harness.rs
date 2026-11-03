// SPDX-License-Identifier: Apache-2.0
//! Injected-error and `max_entities` runners plus one `REDUCED_FIELDS`
//! row check over the writer's default target (#217 §5.0, #232 §5.2).
//! The capacity helper constructs its refusal only after #239 switches
//! the encode path to `EncodeError`, so it is not wired here. Row probes
//! use record-relative channel paths; only the body-colour row runs
//! (layer-bound display colours restructure on every write, and the
//! tessellation normal path matches no decoded golden).
//!
//! A debug build runs the §9.4 fixed sample: the first fixture in path
//! order. `cargo test --release --test contract` runs all.

use std::io::Cursor;
use std::path::PathBuf;

use cadmpeg_ir::codec::write::{Encoder, ReducedStorage};
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_registry::Format;
use cadmpeg_test_support::cross_closure::{cross_request, ReducedProbe};
use cadmpeg_test_support::reduced::reduced_row_findings;
use cadmpeg_test_support::robustness::{
    injected_error_findings, max_entities_findings, standard_injections,
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

/// Every fixture, or in a debug build the first fixture in path order.
fn sampled() -> Vec<PathBuf> {
    let mut paths = fixtures("rhino");
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

fn assert_none(findings: &[String]) {
    assert!(
        findings.is_empty(),
        "{} finding(s):\n{}",
        findings.len(),
        findings.join("\n")
    );
}

/// Injected neutral errors must invalidate the document, yet the write
/// succeeds with a loss naming the affected record (#217 §5.0).
#[test]
fn rhino_injected_errors_name_the_record() {
    let encoder = cadmpeg_registry::build_encoder(Format::Rhino);
    let source = codec("rhino");
    let injections = standard_injections();
    let mut findings = Vec::new();
    for path in sampled() {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        findings.extend(
            injected_error_findings(
                first.ir(),
                encoder.as_ref(),
                cross_request(encoder.as_ref()),
                &injections,
            )
            .into_iter()
            .map(|finding| format!("{}: {finding}", path.display())),
        );
    }
    assert_none(&findings);
}

/// Planning below the golden's record count refuses with the session limit,
/// while the desktop ceiling plans (#217 §5.0).
#[test]
fn rhino_max_entities_refuses_below_count() {
    let encoder = cadmpeg_registry::build_encoder(Format::Rhino);
    let source = codec("rhino");
    let mut findings = Vec::new();
    for path in sampled() {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        let record_count: u64 = first
            .ir()
            .census()
            .values()
            .sum::<usize>() as u64;
        findings.extend(
            max_entities_findings(
                first.ir(),
                encoder.as_ref(),
                cross_request(encoder.as_ref()),
                record_count,
            )
            .into_iter()
            .map(|finding| format!("{}: {finding}", path.display())),
        );
    }
    assert_none(&findings);
}

fn reduced_probes() -> Vec<ReducedProbe> {
    // Record-relative channel paths: colours are structs and the quantum
    // probe is a scalar, so the probe names one channel outright. Only the
    // body colour row runs: layer-bound display colours restructure into
    // object bindings on every write (even unprobed), so the harness's
    // exact-identity lookup cannot address them, and no golden holds the
    // tessellation normal path.
    [("color.r", ReducedStorage::U8Channel)]
        .into_iter()
        .map(|(path, storage)| ReducedProbe {
            path,
            storage,
            normalize: None,
        })
        .collect()
}

/// The first golden in path order holding `path`: the probe needs a witness
/// record, and only `generated_point` carries colours today.
fn witness(path: &str) -> Option<PathBuf> {
    let mut paths = fixtures("rhino");
    paths.sort();
    for candidate in paths {
        let bytes = std::fs::read(&candidate).ok()?;
        let encoder = cadmpeg_registry::build_encoder(Format::Rhino);
        let source = codec("rhino");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        let document = serde_json::to_value(first.ir()).ok()?;
        let wanted = format!(".{path}");
        if document_paths(&document)
            .iter()
            .any(|candidate| candidate.ends_with(&wanted))
        {
            return Some(candidate);
        }
    }
    None
}

/// Collapsed JSON paths with non-null values below the document root.
fn document_paths(document: &serde_json::Value) -> std::collections::BTreeSet<String> {
    fn walk(value: &serde_json::Value, trail: &mut String, out: &mut std::collections::BTreeSet<String>) {
        match value {
            serde_json::Value::Object(fields) => {
                for (key, child) in fields {
                    let base = trail.len();
                    if !trail.is_empty() {
                        trail.push('.');
                    }
                    trail.push_str(key);
                    if !matches!(child, serde_json::Value::Null) {
                        out.insert(trail.clone());
                    }
                    walk(child, trail, out);
                    trail.truncate(base);
                }
            }
            serde_json::Value::Array(items) => {
                let base = trail.len();
                trail.push_str("[]");
                for item in items {
                    walk(item, trail, out);
                }
                trail.truncate(base);
            }
            _ => {}
        }
    }
    let mut out = std::collections::BTreeSet::new();
    walk(document, &mut String::new(), &mut out);
    out
}
/// Every `REDUCED_FIELDS` row with a golden witness stores its reduced value:
/// an unrepresentable probe re-decodes as the stored value, different from
/// the input (#232 §5.2).
#[test]
fn rhino_reduced_rows_hold() {
    let encoder = cadmpeg_registry::build_encoder(Format::Rhino);
    let source = codec("rhino");
    let rhino = codec("rhino");
    let probes = reduced_probes();
    let mut findings = Vec::new();
    for probe in &probes {
        let Some(path) = witness(probe.path) else {
            findings.push(format!("no golden holds the reduced path {}", probe.path));
            continue;
        };
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            findings.push(format!("the {} witness does not decode", path.display()));
            continue;
        };
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            let result = decode(rhino, encoder.as_ref(), written)?;
            Ok(Redecoded {
                members: Vec::new(),
                ir: serde_json::to_value(result.ir()).map_err(|error| error.to_string())?,
                loss_codes: result
                    .report()
                    .losses
                    .iter()
                    .map(|loss| loss.code.to_string())
                    .collect(),
            })
        };
        findings.extend(
            reduced_row_findings(
                first.ir(),
                encoder.as_ref(),
                cross_request(encoder.as_ref()),
                &redecode,
                probe,
                None,
            )
            .into_iter()
            .map(|finding| format!("{}: {finding}", path.display())),
        );
    }
    assert_none(&findings);
}
