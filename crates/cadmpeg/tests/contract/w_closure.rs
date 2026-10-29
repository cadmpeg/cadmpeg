// SPDX-License-Identifier: Apache-2.0
//! W-closure-same (§5.1, #232) for every writer over its fixture goldens.
//!
//! Each writer X writes every fixture of format X whose decode succeeds; the
//! CADIR writer writes every fixture of every format. A debug build (`cargo
//! test-fast`) runs the fixed sample of #216 §9.4: the first fixture of each
//! format in path order. `cargo test --release --test contract` runs all.

use std::io::Cursor;
use std::path::PathBuf;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions, DecodeResult};
use cadmpeg_registry::Format;
use cadmpeg_test_support::writer_closure::{
    edit_findings, with_fidelity, without_fidelity, Redecoded,
};

use super::conformance::fixtures;

const FIXTURE_FORMATS: [&str; 8] = [
    "step", "iges", "rhino", "fcstd", "f3d", "sldprt", "catia", "creo",
];

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

/// The base build that `scripts/base-build.sh` installed (#216 Base build).
fn base_build() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CADMPEG_BASE_BIN")
            .expect("CADMPEG_BASE_BIN is unset; run `eval \"$(scripts/base-build.sh)\"` first"),
    )
}

fn loss_code(loss: &serde_json::Value) -> Option<String> {
    let code = loss.get("code")?;
    Some(format!(
        "{}/{}",
        code.get("namespace")?.as_str()?,
        code.get("code")?.as_str()?
    ))
}

/// Every loss code the base build charges on a base decode golden.
fn base_golden_codes(base: &std::path::Path) -> std::collections::BTreeSet<String> {
    let source = base
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the base build lives at target/base/cadmpeg")
        .join("base-src/crates");
    let mut codes = std::collections::BTreeSet::new();
    let mut pending = vec![source];
    while let Some(directory) = pending.pop() {
        for path in std::fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
        {
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
                && path.components().any(|part| part.as_os_str() == "golden")
            {
                let Ok(value) = serde_json::from_str::<serde_json::Value>(
                    &std::fs::read_to_string(&path).unwrap_or_default(),
                ) else {
                    continue;
                };
                let decode = value.get("decode").unwrap_or(&value);
                for loss in decode
                    .pointer("/report/losses")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    codes.extend(loss_code(loss));
                }
            }
        }
    }
    codes
}

/// A base-build `dump` of one file: its document and its loss codes.
fn base_dump(
    base: &std::path::Path,
    input: &std::path::Path,
    scratch: &std::path::Path,
) -> Result<(serde_json::Value, Vec<String>), String> {
    let output = scratch.join("base.json");
    let status = std::process::Command::new(base)
        .arg("dump")
        .arg(input)
        .arg("-o")
        .arg(&output)
        .output()
        .map_err(|error| error.to_string())?;
    if !status.status.success() {
        return Err(format!(
            "the base build refuses it: {}",
            String::from_utf8_lossy(&status.stderr).trim()
        ));
    }
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&output).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let sidecar: serde_json::Value = serde_json::from_slice(
        &std::fs::read(scratch.join("base.fidelity.json")).unwrap_or_default(),
    )
    .unwrap_or(serde_json::Value::Null);
    let codes = sidecar
        .pointer("/report/losses")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(loss_code)
        .collect();
    Ok((document, codes))
}

fn counts(document: &serde_json::Value) -> std::collections::BTreeMap<String, usize> {
    document
        .get("model")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flatten()
        .map(|(arena, rows)| (arena.clone(), rows.as_array().map_or(0, Vec::len)))
        .collect()
}

/// §5.4 item 2 over the §5.1 outputs of one writer.
fn base_reads(output: Format, format: &str, extension: &str) -> Vec<String> {
    let base = base_build();
    let base_codes = base_golden_codes(&base);
    let encoder = cadmpeg_registry::build_encoder(output);
    let source = codec(format);
    let scratch = tempfile::tempdir().expect("scratch directory");
    let mut findings = Vec::new();
    for path in sampled(format) {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        let Ok(first) = decode(source, encoder.as_ref(), &bytes) else {
            continue;
        };
        let (request, _) =
            cadmpeg_test_support::writer_closure::same_request(encoder.as_ref(), &first);
        let Ok(written) =
            cadmpeg_test_support::writer_closure::write(encoder.as_ref(), &first, false, request)
        else {
            continue;
        };
        let target = scratch.path().join(format!("output.{extension}"));
        std::fs::write(&target, &written.bytes).expect("scratch output");
        let label = path.display();
        let (outcome, codes) = match base_dump(&base, &target, scratch.path()) {
            Ok(dumped) => dumped,
            Err(error) => {
                findings.push(format!("{label}: {error}"));
                continue;
            }
        };
        for code in codes.iter().filter(|code| !base_codes.contains(*code)) {
            findings.push(format!(
                "{label}: the base build charges {code}, which it charges on no base golden"
            ));
        }
        let Ok((original, _)) = base_dump(&base, &path, scratch.path()) else {
            continue;
        };
        let before = serde_json::to_value(first.ir()).expect("CADIR JSON");
        let allowed = cadmpeg_test_support::comparison::allowed_removals_by_arena(&before);
        let written_counts = counts(&outcome);
        for (arena, count) in counts(&original) {
            let floor = count.saturating_sub(allowed.get(&arena).copied().unwrap_or(0));
            let held = written_counts.get(&arena).copied().unwrap_or(0);
            if held < floor {
                findings.push(format!(
                    "{label}: the base decode of the output holds {held} {arena}, below {floor}"
                ));
            }
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
    assert_none(&run(Format::Cadir, &FIXTURE_FORMATS, false));
}

macro_rules! same_format {
    ($($module:ident => $format:literal, $output:ident);* $(;)?) => {
        $(#[cfg(feature = $format)]
        mod $module {
            #[test]
            fn writes_its_fixtures_without_fidelity() {
                super::assert_none(&super::run(cadmpeg_registry::Format::$output, &[$format], false));
            }
        })*
    };
}

macro_rules! same_format_with_fidelity {
    ($($module:ident => $format:literal, $output:ident);* $(;)?) => {
        $(#[cfg(feature = $format)]
        mod $module {
            #[test]
            fn writes_its_fixtures_with_fidelity() {
                super::assert_none(&super::run(cadmpeg_registry::Format::$output, &[$format], true));
            }
        })*
    };
}

same_format! {
    step_w_closure => "step", Step;
    iges_w_closure => "iges", Iges;
    rhino_w_closure => "rhino", Rhino;
    freecad_w_closure => "fcstd", Fcstd;
    f3d_w_closure => "f3d", F3d;
    sldprt_w_closure => "sldprt", Sldprt;
}

same_format_with_fidelity! {
    iges_w_closure_fidelity => "iges", Iges;
    freecad_w_closure_fidelity => "fcstd", Fcstd;
    f3d_w_closure_fidelity => "f3d", F3d;
    sldprt_w_closure_fidelity => "sldprt", Sldprt;
}

macro_rules! edits_reach_the_output {
    ($($module:ident => $format:literal, $output:ident, $namespace:literal, [$($fidelity:literal),*]);* $(;)?) => {
        $(#[cfg(feature = $format)]
        mod $module {
            #[test]
            fn every_edit_reaches_the_output() {
                super::assert_none(&super::edits(
                    cadmpeg_registry::Format::$output, $format, $namespace, &[$($fidelity),*],
                ));
            }
        })*
    };
}

edits_reach_the_output! {
    step_w_closure_edits => "step", Step, "step", [false];
    iges_w_closure_edits => "iges", Iges, "iges", [false, true];
    rhino_w_closure_edits => "rhino", Rhino, "rhino", [false];
    freecad_w_closure_edits => "fcstd", Fcstd, "fcstd", [false, true];
    f3d_w_closure_edits => "f3d", F3d, "f3d", [false, true];
    sldprt_w_closure_edits => "sldprt", Sldprt, "sldprt", [false, true];
}

macro_rules! base_reads_the_outputs {
    ($($module:ident => $format:literal, $output:ident, $extension:literal);* $(;)?) => {
        $(#[cfg(feature = $format)]
        mod $module {
            #[test]
            fn the_base_build_reads_every_output() {
                super::assert_none(&super::base_reads(cadmpeg_registry::Format::$output, $format, $extension));
            }
        })*
    };
}

base_reads_the_outputs! {
    step_w_closure_base => "step", Step, "p21";
    iges_w_closure_base => "iges", Iges, "igs";
    rhino_w_closure_base => "rhino", Rhino, "3dm";
    freecad_w_closure_base => "fcstd", Fcstd, "FCStd";
    f3d_w_closure_base => "f3d", F3d, "f3d";
    sldprt_w_closure_base => "sldprt", Sldprt, "SLDPRT";
}
