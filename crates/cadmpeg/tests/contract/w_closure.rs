// SPDX-License-Identifier: Apache-2.0
//! W-closure-same (§5.1, #232) for the CADIR and STEP writers.
//!
//! Each writer's entry stays on its own line so parallel Phase 4 merges stay
//! clean: following writers add their own `run` and `edits` calls below.
//! A debug build (`cargo test-fast`) runs the fixed sample of #216 §9.4: the
//! first fixture of each format in path order. `cargo test --release --test
//! contract` runs all.

use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::PathBuf;

use cadmpeg_ir::codec::write::{EncodeInput, Encoder, target::TargetRequest};
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_registry::Format;
use cadmpeg_test_support::carried::Carried;
use cadmpeg_test_support::cross_closure::{cross_findings, single_value_paths, CrossConfig};
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
fn sldprt_writes_its_fixtures_without_fidelity() {
    assert_none(&run(Format::Sldprt, &["sldprt"], false));
}

#[test]
fn sldprt_writes_its_fixtures_with_fidelity() {
    assert_none(&run(Format::Sldprt, &["sldprt"], true));
}

#[test]
fn sldprt_off_catalog_sources_write_under_inherit() {
    assert_none(&off_catalog_inherit_writes(Format::Sldprt, "sldprt"));
}

#[test]
fn step_off_catalog_sources_write_under_inherit() {
    assert_none(&off_catalog_inherit_writes(Format::Step, "step"));
}

#[test]
fn iges_writes_its_fixtures_without_fidelity() {
    assert_none(&run(Format::Iges, &["iges"], false));
}

#[test]
fn iges_writes_its_fixtures_with_fidelity() {
    assert_none(&run(Format::Iges, &["iges"], true));
}

#[test]
fn iges_off_catalog_sources_write_under_inherit() {
    assert_none(&off_catalog_inherit_writes(Format::Iges, "iges"));
}

#[test]
fn iges_edits_reach_the_output() {
    assert_none(&edits(Format::Iges, "iges", "iges", &[false, true]));
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

#[test]
fn sldprt_edits_reach_the_output() {
    assert_none(&edits(Format::Sldprt, "sldprt", "sldprt", &[true, false]));
}

#[test]
fn rhino_writes_its_fixtures_without_fidelity() {
    assert_none(&run(Format::Rhino, &["rhino"], false));
}

#[test]
fn rhino_off_catalog_sources_write_under_inherit() {
    assert_none(&off_catalog_inherit_writes(Format::Rhino, "rhino"));
}

#[test]
fn rhino_edits_reach_the_output() {
    assert_none(&edits(Format::Rhino, "rhino", "rhino", &[false]));
}

#[test]
fn f3d_writes_its_fixtures_without_fidelity() {
    assert_none(&run(Format::F3d, &["f3d"], false));
}

#[test]
fn f3d_off_catalog_sources_write_under_inherit() {
    assert_none(&off_catalog_inherit_writes(Format::F3d, "f3d"));
}

#[test]
fn f3d_edits_reach_the_output() {
    assert_none(&edits(Format::F3d, "f3d", "f3d", &[true, false]));
}

#[test]
fn cadir_declares_no_reduced_fields() {
    assert!(cadmpeg_ir::codec::write::REDUCED_FIELDS.is_empty());
}

/// §5.2 cross-closure to CADIR: every golden of every other format writes to
/// CADIR and re-reads with no missing required record, no new loss and no
/// application-private structural name. CADIR carries everything, so the
/// carried sets are the wildcard and the re-read charges no loss.
#[test]
fn cadir_writes_every_foreign_fixture_cross() {
    let encoder = cadmpeg_registry::build_encoder(Format::Cadir);
    let carried = Carried::all();
    let known_losses = std::collections::BTreeSet::new();
    let mut before_documents = Vec::new();
    let mut decoded = Vec::new();
    for format in ["step", "iges", "rhino", "fcstd", "f3d", "sldprt", "catia", "creo"] {
        let source = codec(format);
        for path in sampled(format) {
            let bytes = std::fs::read(&path).expect("fixture bytes");
            let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
                continue;
            };
            before_documents.push(
                serde_json::to_value(first.ir()).expect("CADIR JSON"),
            );
            decoded.push((path, first));
        }
    }
    let fixed_paths = single_value_paths(&before_documents);
    assert!(
        !decoded.is_empty(),
        "the cross sample decodes no fixture"
    );
    let config = CrossConfig {
        carried: &carried,
        fixed_paths: &fixed_paths,
        known_losses: &known_losses,
        reduced: &[],
        members: &[],
    };
    let mut findings = Vec::new();
    for (path, first) in &decoded {
        let redecode = |written: &[u8]| -> Result<Redecoded, String> {
            let text = std::str::from_utf8(written).map_err(|error| error.to_string())?;
            let ir = cadmpeg_ir::CadIr::from_json(text).map_err(|error| error.to_string())?;
            Ok(Redecoded {
                ir: serde_json::to_value(&ir).map_err(|error| error.to_string())?,
                loss_codes: BTreeSet::new(),
                members: Vec::new(),
            })
        };
        findings.extend(
            cross_findings(first, encoder.as_ref(), &redecode, &config)
                .into_iter()
                .map(|finding| format!("{}: {finding}", path.display())),
        );
    }
    assert_none(&findings);
}

#[test]
fn cadir_reports_no_loss_and_no_synthesis_cause() {
    let encoder = cadmpeg_registry::build_encoder(Format::Cadir);
    let mut findings = Vec::new();
    for format in ["step", "iges", "rhino", "fcstd", "f3d", "sldprt"] {
        let source = codec(format);
        for path in sampled(format) {
            let bytes = std::fs::read(&path).expect("fixture bytes");
            let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
                continue;
            };
            let Ok(written) = cadmpeg_test_support::writer_closure::write(
                encoder.as_ref(),
                &first,
                false,
                cadmpeg_ir::codec::write::target::TargetRequest::Inherit,
            ) else {
                findings.push(format!("{}: cadir refuses", path.display()));
                continue;
            };
            if !written.report.losses.is_empty() {
                findings.push(format!(
                    "{}: cadir charges {} loss(es)",
                    path.display(),
                    written.report.losses.len()
                ));
            }
            if written.report.synthesis_cause().is_some() {
                findings.push(format!("{}: cadir carries a synthesis cause", path.display()));
            }
            let before: serde_json::Value =
                serde_json::to_value(first.ir()).expect("CADIR JSON");
            let native_before: std::collections::BTreeSet<(String, String)> = before
                .get("native")
                .and_then(serde_json::Value::as_object)
                .into_iter()
                .flat_map(|namespaces| namespaces.iter())
                .filter_map(|(namespace, arenas)| {
                    Some(
                        arenas
                            .as_object()?
                            .keys()
                            .map(move |arena| (namespace.clone(), arena.clone())),
                    )
                })
                .flatten()
                .collect();
            let native_after: std::collections::BTreeSet<(String, String)> = written
                .report
                .native_dispositions
                .namespaces
                .iter()
                .flat_map(|(namespace, declared)| {
                    declared
                        .arenas
                        .keys()
                        .map(move |arena| (namespace.clone(), arena.clone()))
                })
                .collect();
            if native_before != native_after {
                findings.push(format!("{}: cadir native registry differs", path.display()));
            }
            for (namespace, arena) in &native_after {
                if written.report.native_dispositions.disposition(namespace, arena)
                    != cadmpeg_ir::codec::write::NativeDisposition::Written
                {
                    findings.push(format!(
                        "{}: cadir declares {namespace}.{arena} as not written",
                        path.display()
                    ));
                }
            }
        }
    }
    assert_none(&findings);
}
