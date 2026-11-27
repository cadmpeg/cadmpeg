// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange;
use cadmpeg_ir::topology::PcurveUse;
use cadmpeg_ir::{
    math::{Point2, Point3},
    report::check::Check,
    CadIr,
};
use cadmpeg_registry::{repro::Bundle, InputCatalog};
use clap::Parser;

#[test]
fn repro_cli_cuts_report_and_checks_normal_document_bundle() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("model.json");
    let report = directory.path().join("report.json");
    let output = directory.path().join("bundle.json");
    let mut document = cadmpeg_ir::examples::unit_cube().unwrap();
    document.geometry.points[0].set_position(Point3::new(50.0, 0.0, 0.0).try_into().unwrap());
    std::fs::write(&input, json::to_vec(&document).unwrap()).unwrap();
    let validation = cadmpeg_ir::validate_neutral(&document, Vec::new()).unwrap();
    let index = validation
        .findings
        .iter()
        .position(|finding| finding.check == Check::GeometricConsistency)
        .unwrap();
    let payload = cadmpeg_core::json!({"ir_version": cadmpeg_ir::document::IR_VERSION,
        "command": "check", "input_path": input, "check_report": validation});
    std::fs::write(&report, json::to_vec(&payload).unwrap()).unwrap();
    let cli = crate::Cli::try_parse_from([
        "cadmpeg",
        "repro",
        "--from",
        report.to_str().unwrap(),
        "--finding",
        &index.to_string(),
        "-o",
        output.to_str().unwrap(),
    ])
    .unwrap();
    let crate::Command::Repro(args) = cli.command else {
        panic!("repro command expected");
    };
    crate::repro::run(&InputCatalog::with_builtins(), &args).unwrap();
    let bytes = std::fs::read(&output).unwrap();
    let bundle: Bundle = json::from_slice(&bytes).unwrap();
    let parsed: CadIr = json::from_slice(&bytes).unwrap();
    assert!(parsed.fragment);
    assert!(parsed.repro.is_some());
    assert!(cadmpeg_registry::repro::check(&bundle)
        .iter()
        .any(|finding| bundle.signature.reproduces(finding)));
    let fragment_report = directory.path().join("fragment-report.json");
    let cli = crate::Cli::try_parse_from([
        "cadmpeg",
        "check",
        "--fragment",
        output.to_str().unwrap(),
        "--report",
        fragment_report.to_str().unwrap(),
    ])
    .unwrap();
    let crate::Command::Check {
        fragment: true,
        report,
        ..
    } = cli.command
    else {
        panic!("fragment check expected");
    };
    let result = crate::repro::check_fragment(&output, false, report.0.as_ref());
    assert_eq!(result.unwrap_err().exit_code(), 1);
    let envelope: serde_json::Value =
        json::from_slice(&std::fs::read(fragment_report).unwrap()).unwrap();
    assert_eq!(envelope["status"], "refused");
    assert_eq!(envelope["refusal"]["code"], "check_failed");
    assert!(crate::repro::run(&InputCatalog::with_builtins(), &args).is_err());
}

#[test]
fn repro_cli_nonreproducing_cut_is_written_before_optional_failure() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("model.json");
    let report = directory.path().join("report.json");
    let document = cadmpeg_ir::examples::unit_cube().unwrap();
    std::fs::write(&input, json::to_vec(&document).unwrap()).unwrap();
    let finding = cadmpeg_ir::report::check::Finding {
        rule: cadmpeg_ir::rule_name!("cadmpeg/test.synthetic-finding-requiring-additional-context"),
        check: Check::GeometricConsistency,
        severity: cadmpeg_ir::report::Severity::Error,
        entity: Some(document.geometry.bodies[0].id.to_string()),
        related: Vec::new(),
        message: "synthetic finding requiring additional context".into(),
    };
    let payload =
        cadmpeg_core::json!({"input_path": input, "check_report": {"findings": [finding]}});
    std::fs::write(&report, json::to_vec(&payload).unwrap()).unwrap();
    for strict in [false, true] {
        let output = directory.path().join(format!("bundle-{strict}.json"));
        let mut argv = vec![
            "cadmpeg",
            "repro",
            "--from",
            report.to_str().unwrap(),
            "--finding",
            "0",
            "-o",
            output.to_str().unwrap(),
        ];
        if strict {
            argv.push("--require-reproduction");
        }
        let cli = crate::Cli::try_parse_from(argv).unwrap();
        let crate::Command::Repro(args) = cli.command else {
            panic!("repro command expected");
        };
        assert_eq!(
            crate::repro::run(&InputCatalog::with_builtins(), &args).is_err(),
            strict
        );
        let bundle: Bundle = json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert!(bundle.incomplete_reason.is_some());
        assert_eq!(bundle.document.geometry.bodies.len(), 1);
    }
}

fn holds_negative_zero(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Number(number) => number
            .as_f64()
            .is_some_and(|number| number == 0.0 && number.is_sign_negative()),
        serde_json::Value::Array(items) => items.iter().any(holds_negative_zero),
        serde_json::Value::Object(members) => members.values().any(holds_negative_zero),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::String(_) => {
            false
        }
    }
}

#[test]
fn repro_bundle_holds_the_one_zero() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("model.json");
    let report = directory.path().join("report.json");
    let output = directory.path().join("bundle.json");
    let mut document = cadmpeg_ir::examples::unit_cube().unwrap();
    document.geometry.points[0].set_position(Point3::new(50.0, 0.0, 0.0).try_into().unwrap());
    let validation = cadmpeg_ir::validate_neutral(&document, Vec::new()).unwrap();
    let finding = validation
        .findings
        .into_iter()
        .find(|finding| finding.check == Check::GeometricConsistency)
        .unwrap();
    let pcurve = cadmpeg_ir::ids::PcurveId::mint("synthetic:use:pcurve#1").unwrap();
    document.geometry.pcurves.push(Pcurve {
        id: pcurve.clone(),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
        ),
        metadata: PcurveMetadata::default(),
        native_ref: None,
    });
    for coedge in &mut document.geometry.coedges {
        coedge.pcurves.push(PcurveUse {
            pcurve: pcurve.clone(),
            isoparametric: None,
            parameter_range: Some(DirectedParameterRange::new([-0.0, 1.0]).unwrap()),
        });
    }
    document.finalize();
    let input_bytes = json::to_vec(&document).unwrap();
    assert!(holds_negative_zero(
        &json::from_slice(&input_bytes).unwrap()
    ));
    std::fs::write(&input, input_bytes).unwrap();
    let payload =
        cadmpeg_core::json!({"input_path": input, "check_report": {"findings": [finding]}});
    std::fs::write(&report, json::to_vec(&payload).unwrap()).unwrap();
    let cli = crate::Cli::try_parse_from([
        "cadmpeg",
        "repro",
        "--from",
        report.to_str().unwrap(),
        "--finding",
        "0",
        "-o",
        output.to_str().unwrap(),
    ])
    .unwrap();
    let crate::Command::Repro(args) = cli.command else {
        panic!("repro command expected");
    };
    crate::repro::run(&InputCatalog::with_builtins(), &args).unwrap();
    let bytes = std::fs::read(&output).unwrap();
    let bundle: Bundle = json::from_slice(&bytes).unwrap();
    assert!(bundle
        .document
        .geometry
        .coedges
        .iter()
        .flat_map(|coedge| &coedge.pcurves)
        .any(|use_| use_.parameter_range.is_some()));
    assert!(!holds_negative_zero(&json::from_slice(&bytes).unwrap()));
}
