// SPDX-License-Identifier: Apache-2.0
//! W-closure-same (§5.1, #232) for the CADIR and STEP writers.
//!
//! Each writer's entry stays on its own line so parallel Phase 4 merges stay
//! clean: following writers add their own `run` and `edits` calls below.
//! A debug build (`cargo test-fast`) runs the fixed sample of #216 §9.4: the
//! first fixture of each format in path order. `cargo test --release --test
//! contract` runs all.

use std::io::Cursor;
use std::path::PathBuf;

use cadmpeg_ir::codec::write::{EncodeInput, Encoder, target::TargetRequest};
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_registry::Format;
use cadmpeg_test_support::writer_closure::{
    edit_findings, with_fidelity, without_fidelity, Redecoded,
};

use super::conformance::fixtures;

fn codec(format: &str) -> &'static dyn Codec {
    let catalog = Box::leak(Box::new(cadmpeg_registry::InputCatalog::with_builtins()));
    catalog
        .descriptors()
        .find(|descriptor| descriptor.format_id().as_str() == format)
        .and_then(cadmpeg_registry::InputDescriptor::codec)
        .unwrap_or_else(|| panic!("{format} has no codec in this build"))
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

fn members(bytes: &[u8]) -> Vec<String> {
    let Ok(archive) = zip::ZipArchive::new(Cursor::new(bytes)) else {
        return Vec::new();
    };
    archive
        .file_names()
        .filter_map(Result::ok)
        .map(std::borrow::Cow::into_owned)
        .collect()
}

fn redecoded(result: &DecodeResult, bytes: &[u8]) -> Result<Redecoded, String> {
    Ok(Redecoded {
        members: members(bytes),
        ir: serde_json::to_value(result.ir()).map_err(|error| error.to_string())?,
        loss_codes: result
            .report()
            .losses
            .iter()
            .map(|loss| loss.code.to_string())
            .collect(),
    })
}

/// Every fixture of `format`, or in a debug build its first fixture.
fn sampled(format: &str) -> Vec<PathBuf> {
    let mut paths = fixtures(format);
    if cfg!(debug_assertions) {
        paths.truncate(1);
    }
    paths
}

fn inputs(formats: &[&str]) -> Vec<(&'static dyn Codec, PathBuf)> {
    formats
        .iter()
        .flat_map(|format| {
            sampled(format)
                .into_iter()
                .map(move |path| (codec(format), path))
        })
        .collect()
}

fn run(output: Format, formats: &[&str], fidelity: bool) -> Vec<String> {
    let encoder = cadmpeg_registry::build_encoder(output);
    let mut findings = Vec::new();
    for (source, path) in inputs(formats) {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            if output == Format::Cadir {
                let text = std::str::from_utf8(written).map_err(|error| error.to_string())?;
                let ir = cadmpeg_ir::CadIr::from_json(text).map_err(|error| error.to_string())?;
                return Ok(Redecoded {
                    ir: serde_json::to_value(&ir).map_err(|error| error.to_string())?,
                    loss_codes: first
                        .report()
                        .losses
                        .iter()
                        .map(|loss| loss.code.to_string())
                        .collect(),
                    members: Vec::new(),
                });
            }
            redecoded(&decode(source, encoder.as_ref(), written)?, written)
        };
        let checked = if fidelity {
            with_fidelity(&first, encoder.as_ref(), &redecode)
        } else {
            without_fidelity(&first, encoder.as_ref(), &redecode)
        };
        findings.extend(
            checked
                .into_iter()
                .map(|finding| format!("{}: {finding}", path.display())),
        );
    }
    findings
}

/// §5.1(c) over the fixtures of one writer's format, in path order.
fn edits(output: Format, format: &str, namespace: &str, fidelity: &[bool]) -> Vec<String> {
    let encoder = cadmpeg_registry::build_encoder(output);
    let source = codec(format);
    let mut covered = std::collections::BTreeSet::new();
    let mut findings = Vec::new();
    for path in sampled(format) {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            redecoded(&decode(source, encoder.as_ref(), written)?, written)
        };
        let found = edit_findings(
            &first,
            encoder.as_ref(),
            &redecode,
            namespace,
            fidelity,
            &mut covered,
        );
        findings.extend(
            found
                .into_iter()
                .map(|finding| format!("{}: {finding}", path.display())),
        );
    }
    findings
}

/// Off-catalog goldens of one format still write under `Inherit` (§5.0).
fn off_catalog_inherit_writes(output: Format, format: &str) -> Vec<String> {
    let encoder = cadmpeg_registry::build_encoder(output);
    let source = codec(format);
    let catalog = encoder.targets();
    let mut findings = Vec::new();
    for path in sampled(format) {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        let recorded = first
            .ir()
            .source
            .as_ref()
            .filter(|source| source.format() == encoder.id().as_str())
            .and_then(|source| source.dialect())
            .map(|matched| matched.dialect().as_str().to_owned());
        let off_catalog = recorded.as_deref().is_some_and(|dialect| {
            !catalog.is_empty() && catalog.find(dialect).is_none()
        }) || (format == "step"
            && recorded.as_deref().is_some_and(|dialect| {
                dialect == "step:ap242" || dialect == "step:unknown"
            }));
        if !off_catalog {
            continue;
        }
        let written = encoder
            .plan(
                EncodeInput::new(first.ir(), None),
                TargetRequest::Inherit,
            )
            .map(|plan| {
                let mut bytes = Vec::new();
                plan.write_to(&mut bytes).map(|_| bytes)
            });
        if let Err(error) = written {
            findings.push(format!("{}: Inherit refuses an off-catalog source: {error:?}", path.display()));
        }
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
fn cadir_writes_every_fixture_without_fidelity() {
    assert_none(&run(
        Format::Cadir,
        &["step", "iges", "rhino", "fcstd", "f3d", "sldprt", "catia", "creo"],
        false,
    ));
}

#[test]
fn step_writes_its_fixtures_without_fidelity() {
    assert_none(&run(Format::Step, &["step"], false));
}

#[test]
fn step_off_catalog_sources_write_under_inherit() {
    assert_none(&off_catalog_inherit_writes(Format::Step, "step"));
}

#[test]
fn cadir_edits_reach_the_output() {
    let encoder = cadmpeg_registry::build_encoder(Format::Cadir);
    let mut covered = std::collections::BTreeSet::new();
    let mut findings = Vec::new();
    for format in ["step", "iges", "rhino", "fcstd", "f3d", "sldprt"] {
        let source = codec(format);
        for path in sampled(format) {
            let bytes = std::fs::read(&path).expect("fixture bytes");
            let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
                continue;
            };
            let redecode = |written: &[u8]| -> Result<Redecoded, String> {
                let text = std::str::from_utf8(written).map_err(|error| error.to_string())?;
                let ir = cadmpeg_ir::CadIr::from_json(text).map_err(|error| error.to_string())?;
                Ok(Redecoded {
                    ir: serde_json::to_value(&ir).map_err(|error| error.to_string())?,
                    loss_codes: first
                        .report()
                        .losses
                        .iter()
                        .map(|loss| loss.code.to_string())
                        .collect(),
                    members: Vec::new(),
                })
            };
            findings.extend(
                edit_findings(&first, encoder.as_ref(), &redecode, "cadir", &[false], &mut covered)
                    .into_iter()
                    .map(|finding| format!("{}: {finding}", path.display())),
            );
        }
    }
    assert_none(&findings);
}

#[test]
fn step_edits_reach_the_output() {
    assert_none(&edits(Format::Step, "step", "step", &[false]));
}
