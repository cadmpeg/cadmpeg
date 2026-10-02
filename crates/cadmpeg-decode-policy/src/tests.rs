// SPDX-License-Identifier: Apache-2.0
use std::process::Command;

#[test]
#[ignore = "subprocess compiler entry point"]
fn fixture_child() {
    let path = std::env::var("CADMPEG_POLICY_INPUT").expect("fixture path");
    let directory = std::env::var("CADMPEG_POLICY_OUTPUT").expect("fixture output");
    let args = vec!["rustc".to_owned(), path, "--crate-type=lib".to_owned(), "--edition=2021".to_owned(), "--emit=metadata".to_owned(), "--out-dir".to_owned(), directory];
    std::process::exit(i32::from(crate::run(&args)));
}

fn check_fixture(name: &str) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("fixtures").join(format!("{name}.rs"));
    let output_dir = root.join("target/fixtures").join(name);
    std::fs::create_dir_all(&output_dir).expect("fixture directory");
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", "tests::fixture_child", "--ignored", "--nocapture"])
        .env("CADMPEG_POLICY_FIXTURE", "1")
        .env("CADMPEG_POLICY_INPUT", &path)
        .env("CADMPEG_POLICY_OUTPUT", &output_dir)
        .output().expect("fixture compiler");
    let actual = String::from_utf8(output.stdout).expect("diagnostics UTF-8");
    let mut findings = Vec::new();
    for line in actual.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() == 4 { findings.push((fields[2].parse::<usize>().expect("line number"), fields[0].to_owned())); }
    }
    let source = std::fs::read_to_string(path).expect("fixture source");
    let expected: Vec<_> = source.lines().enumerate().filter_map(|(index, line)| line.split_once("// finding: ").map(|(_, rule)| (index + 1, rule.trim().to_owned()))).collect();
    findings.sort();
    assert_eq!(findings, expected, "stdout: {actual}\nstderr: {}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.status.code(), Some(i32::from(!expected.is_empty())), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn typed_allocation_shapes() { check_fixture("allocation"); }
