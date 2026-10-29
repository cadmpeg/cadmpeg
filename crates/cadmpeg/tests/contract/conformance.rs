// SPDX-License-Identifier: Apache-2.0
//! The §9.3 cross-codec conformance checks (#219) over every golden.
//!
//! Fixture-backed goldens decode their fixture at `Full` and at `Geometry`.
//! Code-built goldens are checked at `Full` from their committed snapshot.

use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_test_support::data_checks::conformance::{
    b1_unrepresented, dangling_token_findings, scope_findings,
};
use cadmpeg_test_support::data_checks::decode_goldens;
use serde_json::Value;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn decode_findings(
    document: &Value,
    report: &Value,
    fidelity: &Value,
    requested: &[&str],
) -> Vec<String> {
    let mut findings = scope_findings(document, report, requested);
    findings.extend(dangling_token_findings(document));
    let derived = b1_unrepresented(document, fidelity);
    let stored: BTreeSet<&str> = fidelity
        .get("unrepresented")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if stored != derived {
        findings.push(format!(
            "unrepresented {stored:?} differs from the B1 derivation {derived:?}"
        ));
    }
    findings
}

pub(super) fn fixtures(format: &str) -> Vec<PathBuf> {
    let root = workspace();
    let (directory, extension) = match format {
        "step" => (
            root.join("crates/cadmpeg-codec-step/tests/fixtures"),
            Some("p21"),
        ),
        "fcstd" => (root.join("corpus/freecad_fcstd/fixtures"), Some("FCStd")),
        _ => (
            root.join(format!(
                "crates/cadmpeg-codec-{format}/tests/golden/fixtures"
            )),
            None,
        ),
    };
    let mut found = Vec::new();
    let mut pending = vec![directory];
    while let Some(directory) = pending.pop() {
        for path in std::fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
        {
            if path.is_dir() {
                pending.push(path);
            } else if extension
                .is_none_or(|wanted| path.extension().is_some_and(|ext| ext == wanted))
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

fn fixture_findings(format: &str) -> Vec<String> {
    let catalog = cadmpeg_registry::InputCatalog::with_builtins();
    let codec = catalog
        .descriptors()
        .find(|descriptor| descriptor.format_id().as_str() == format)
        .and_then(cadmpeg_registry::InputDescriptor::codec)
        .unwrap_or_else(|| panic!("{format} has no codec in this build"));
    let mut findings = Vec::new();
    let paths = fixtures(format);
    assert!(!paths.is_empty(), "{format} has no fixtures");
    for path in paths {
        let bytes = std::fs::read(&path).expect("fixture bytes");
        for (depth, requested) in [
            (DecodeDepth::Full, &["model", "design"][..]),
            (DecodeDepth::Geometry, &["model"][..]),
        ] {
            let options = DecodeOptions {
                depth,
                ..DecodeOptions::default()
            };
            let Ok(decoded) = codec.decode(&mut Cursor::new(&bytes), &options) else {
                continue;
            };
            let document = serde_json::to_value(decoded.ir()).expect("CADIR JSON");
            let report = serde_json::to_value(decoded.report()).expect("report JSON");
            let fidelity = serde_json::to_value(decoded.source_fidelity()).expect("fidelity JSON");
            findings.extend(
                decode_findings(&document, &report, &fidelity, requested)
                    .into_iter()
                    .map(|finding| format!("{} at {}: {finding}", path.display(), depth.as_str())),
            );
        }
    }
    findings
}

fn golden_findings(crate_name: &str) -> Vec<String> {
    let mut findings = Vec::new();
    for golden in decode_goldens(crate_name) {
        let text = std::fs::read_to_string(workspace().join(&golden.path)).expect("golden text");
        let value: Value = serde_json::from_str(&text).expect("golden JSON");
        let report = value
            .get("decode")
            .unwrap_or(&value)
            .get("report")
            .cloned()
            .unwrap_or(Value::Null);
        findings.extend(
            decode_findings(&golden.ir, &report, &golden.fidelity, &["model", "design"])
                .into_iter()
                .map(|finding| format!("{}: {finding}", golden.path)),
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

macro_rules! golden_conformance {
    ($($module:ident => $crate_name:literal, $feature:literal);* $(;)?) => {
        $(#[cfg(feature = $feature)]
        mod $module {
            #[test]
            fn goldens_conform_at_full() {
                super::assert_none(&super::golden_findings($crate_name));
            }
        })*
    };
}

macro_rules! fixture_conformance {
    ($($module:ident => $format:literal);* $(;)?) => {
        $(#[cfg(feature = $format)]
        mod $module {
            #[test]
            fn fixtures_conform_at_full_and_geometry() {
                super::assert_none(&super::fixture_findings($format));
            }
        })*
    };
}

golden_conformance! {
    sat_conformance => "cadmpeg-codec-sat", "sat";
    step_conformance => "cadmpeg-codec-step", "step";
    iges_conformance => "cadmpeg-codec-iges", "iges";
    rhino_conformance => "cadmpeg-codec-rhino", "rhino";
    freecad_conformance => "cadmpeg-codec-freecad", "fcstd";
    f3d_conformance => "cadmpeg-codec-f3d", "f3d";
    sldprt_conformance => "cadmpeg-codec-sldprt", "sldprt";
    inventor_conformance => "cadmpeg-codec-inventor", "inventor";
    catia_conformance => "cadmpeg-codec-catia", "catia";
    creo_conformance => "cadmpeg-codec-creo", "creo";
    nx_conformance => "cadmpeg-codec-nx", "nx";
}

fixture_conformance! {
    step_fixture_conformance => "step";
    iges_fixture_conformance => "iges";
    rhino_fixture_conformance => "rhino";
    freecad_fixture_conformance => "fcstd";
    f3d_fixture_conformance => "f3d";
    sldprt_fixture_conformance => "sldprt";
    catia_fixture_conformance => "catia";
    creo_fixture_conformance => "creo";
}
