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
