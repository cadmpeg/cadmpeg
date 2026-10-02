// SPDX-License-Identifier: Apache-2.0
use std::process::Command;

#[test]
#[ignore = "subprocess compiler entry point"]
fn fixture_child() {
    let path = std::env::var("CADMPEG_POLICY_INPUT").expect("fixture path");
    let directory = std::env::var("CADMPEG_POLICY_OUTPUT").expect("fixture output");
    let mut args = vec![
        "rustc".to_owned(),
        path,
        "--crate-type=lib".to_owned(),
        "--edition=2021".to_owned(),
        "--emit=metadata".to_owned(),
        "--out-dir".to_owned(),
        directory,
    ];
    if let Ok(dependency) = std::env::var("CADMPEG_POLICY_DEPENDENCY") {
        args.extend(["--extern".to_owned(), format!("cadmpeg_core={dependency}")]);
    }
    std::process::exit(i32::from(crate::run(&args)));
}

fn check_fixture(name: &str) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("fixtures").join(format!("{name}.rs"));
    let output_dir = root.join("target/fixtures").join(name);
    std::fs::create_dir_all(&output_dir).expect("fixture directory");
    let mut command = Command::new(std::env::current_exe().expect("test binary"));
    if name == "imported" {
        let dependency = output_dir.join("libcadmpeg_core.rlib");
        let status = Command::new("rustc")
            .args(["+nightly-2026-09-08", "--crate-name=cadmpeg_core", "--crate-type=rlib", "--edition=2021"])
            .arg(root.join("fixtures/imported_dependency.rs"))
            .arg("-o").arg(&dependency).status().expect("dependency compiler");
        assert!(status.success());
        command.env("CADMPEG_POLICY_DEPENDENCY", dependency);
    }
    let output = command
        .args([
            "--exact",
            "integration_tests::fixture_child",
            "--ignored",
            "--nocapture",
        ])
        .env("CADMPEG_POLICY_FIXTURE", "1")
        .env("CADMPEG_POLICY_INPUT", &path)
        .env("CADMPEG_POLICY_OUTPUT", &output_dir)
        .output()
        .expect("fixture compiler");
    let actual = String::from_utf8(output.stdout).expect("diagnostics UTF-8");
    let mut findings = Vec::new();
    for line in actual.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() == 4
            && (if matches!(name, "edges" | "modular" | "external" | "generic" | "imported") {
                true
            } else if name.starts_with("work") {
                fields[0] != "uncharged_decode_allocation"
            } else {
                fields[0] != "uncharged_decode_work"
            })
        {
            findings.push((
                fields[2].parse::<usize>().expect("line number"),
                fields[0].to_owned(),
            ));
        }
    }
    let source = std::fs::read_to_string(path).expect("fixture source");
    let mut expected: Vec<_> = source
        .lines()
        .enumerate()
        .flat_map(|(index, line)| {
            line.split_once("// finding: ")
                .into_iter()
                .flat_map(move |(code, rules)| {
                    // rustfmt places loop-header markers inside the body and
                    // tail-expression markers after the closing brace.
                    let line = if code.trim().is_empty() || code.trim() == "}" {
                        index
                    } else {
                        index + 1
                    };
                    rules
                        .split(", ")
                        .map(move |rule| (line, rule.trim().to_owned()))
                })
        })
        .collect();
    expected.sort();
    findings.sort();
    assert_eq!(
        findings,
        expected,
        "stdout: {actual}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.status.code(),
        Some(i32::from(!expected.is_empty())),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn typed_allocation_shapes() {
    check_fixture("allocation");
}

#[test]
fn typed_collection_shapes() {
    check_fixture("collections");
}

#[test]
fn typed_work_shapes() {
    check_fixture("work");
}

#[test]
fn unresolved_shapes() {
    check_fixture("unknown");
}

#[test]
fn context_scope() {
    check_fixture("scope");
}

#[test]
fn writer_exclusion() {
    check_fixture("writer");
}

#[test]
fn scan_hash_copy_shapes() {
    check_fixture("work_operations");
}

#[test]
fn mutation_and_deferred_scans() {
    check_fixture("work_flow");
}

#[test]
fn charge_proof_edges() {
    check_fixture("edges");
}

#[test]
fn modular_body_proof() {
    check_fixture("modular");
}

#[test]
fn external_operation_summaries() {
    check_fixture("external");
}

#[test]
fn concrete_generic_instantiations() {
    check_fixture("generic");
}

#[test]
fn imported_generic_instantiations() {
    check_fixture("imported");
}
