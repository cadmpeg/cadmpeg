// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::{math::Point3, report::check::Check, CadIr};
use cadmpeg_registry::{repro::Bundle, InputCatalog};
use clap::Parser;

#[test]
fn repro_cli_cuts_report_and_checks_normal_document_bundle() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("model.json");
    let report = directory.path().join("report.json");
    let output = directory.path().join("bundle.json");
    let mut document = cadmpeg_ir::examples::unit_cube().unwrap();
    document.model.points[0].set_position(Point3::new(50.0, 0.0, 0.0).try_into().unwrap());
    std::fs::write(&input, serde_json::to_vec(&document).unwrap()).unwrap();
    let validation = cadmpeg_ir::validate_neutral(&document, Vec::new()).unwrap();
    let index = validation
        .findings
        .iter()
        .position(|finding| finding.check == Check::GeometricConsistency)
        .unwrap();
    let payload = serde_json::json!({"ir_version": cadmpeg_ir::document::IR_VERSION,
        "command": "check", "input_path": input, "check_report": validation});
    std::fs::write(&report, serde_json::to_vec(&payload).unwrap()).unwrap();
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
    let bundle: Bundle = serde_json::from_slice(&bytes).unwrap();
    let parsed: CadIr = serde_json::from_slice(&bytes).unwrap();
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
        serde_json::from_slice(&std::fs::read(fragment_report).unwrap()).unwrap();
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
    std::fs::write(&input, serde_json::to_vec(&document).unwrap()).unwrap();
    let finding = cadmpeg_ir::report::check::Finding {
        check: Check::GeometricConsistency,
        severity: cadmpeg_ir::report::Severity::Error,
        entity: Some(document.model.bodies[0].id.to_string()),
        related: Vec::new(),
        message: "synthetic finding requiring additional context".into(),
    };
    let payload = serde_json::json!({"input_path": input, "check_report": {"findings": [finding]}});
    std::fs::write(&report, serde_json::to_vec(&payload).unwrap()).unwrap();
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
        let bundle: Bundle = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert!(bundle.incomplete_reason.is_some());
        assert_eq!(bundle.document.model.bodies.len(), 1);
    }
}
