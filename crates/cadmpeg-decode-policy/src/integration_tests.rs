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
        for dependency in dependency.split(';') {
            args.extend(["--extern".to_owned(), dependency.to_owned()]);
        }
        if let Ok(directory) = std::env::var("CADMPEG_POLICY_DEPENDENCY_DIR") {
            for directory in std::env::split_paths(&directory) {
                args.extend([
                    "-L".to_owned(),
                    format!("dependency={}", directory.display()),
                ]);
            }
        }
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
        let dependency = output_dir.join("libcadmpeg_core.rmeta");
        let status = Command::new("rustc")
            .args([
                "+nightly-2026-09-08",
                "--crate-name=cadmpeg_core",
                "--crate-type=lib",
                "--edition=2021",
                "--emit=metadata",
                "-Zalways-encode-mir",
            ])
            .arg(root.join("fixtures/imported_dependency.rs"))
            .arg("-o")
            .arg(&dependency)
            .status()
            .expect("dependency compiler");
        assert!(status.success());
        command.env(
            "CADMPEG_POLICY_DEPENDENCY",
            format!("cadmpeg_core={}", dependency.display()),
        );
    }
    if matches!(name, "thirdparty" | "serde") {
        let executable = std::env::current_exe().expect("test executable");
        let target = executable
            .ancestors()
            .find(|path| path.file_name().is_some_and(|name| name == "debug"))
            .expect("debug artifact directory");
        let mut directories = vec![target.join("deps")];
        for package in std::fs::read_dir(target.join("build")).expect("package artifacts") {
            for fingerprint in std::fs::read_dir(package.expect("package entry").path())
                .expect("package fingerprints")
            {
                let directory = fingerprint.expect("fingerprint entry").path().join("out");
                if directory.is_dir() {
                    directories.push(directory);
                }
            }
        }
        let mut dependencies = Vec::new();
        for name in ["roxmltree", "serde_json", "serde"] {
            let prefix = format!("lib{name}-");
            let library = directories
                .iter()
                .filter(|directory| directory.is_dir())
                .flat_map(|directory| std::fs::read_dir(directory).expect("dependency listing"))
                .map(|entry| entry.expect("dependency entry").path())
                .filter(|path| {
                    path.file_name()
                        .is_some_and(|file| file.to_string_lossy().starts_with(&prefix))
                        && path
                            .extension()
                            .is_some_and(|extension| extension == "rmeta")
                })
                .max_by_key(|path| {
                    std::fs::metadata(path)
                        .and_then(|metadata| metadata.modified())
                        .expect("dependency modification time")
                })
                .expect("fixture dependency library");
            dependencies.push(format!("{name}={}", library.display()));
        }
        command.env("CADMPEG_POLICY_DEPENDENCY", dependencies.join(";"));
        command.env(
            "CADMPEG_POLICY_DEPENDENCY_DIR",
            std::env::join_paths(directories).expect("dependency paths"),
        );
    }
    if name == "external" {
        command.env("CADMPEG_POLICY_EXTERNALS", "1");
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
    if matches!(name, "generic" | "imported") {
        assert!(
            actual
                .lines()
                .any(|line| line.starts_with("uncharged_decode_allocation\t")
                    && line.contains("concrete instantiation")
                    && line.contains("String")),
            "{actual}"
        );
    }
    if name == "external" {
        for operation in ["abs", "first", "parse", "eq_ignore_ascii_case", "from_fn"] {
            assert!(
                actual
                    .lines()
                    .any(|line| line.starts_with("external_operation\t")
                        && line
                            .split('\t')
                            .nth(1)
                            .is_some_and(|path| path.ends_with(&format!("::{operation}")))
                        && !line.contains("MISSING")),
                "missing inventory cost for {operation}: {actual}"
            );
        }
        assert!(
            actual
                .lines()
                .any(|line| line.starts_with("external_operation\t")
                    && line.contains("std::thread::current\tMISSING")),
            "{actual}"
        );
    }
    let mut findings = Vec::new();
    for line in actual.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() == 4
            && matches!(
                fields[0],
                "uncharged_decode_allocation" | "uncharged_decode_work" | "unproven_decode_charge"
            )
            && (if matches!(
                name,
                "edges"
                    | "modular"
                    | "external"
                    | "generic"
                    | "imported"
                    | "dominance"
                    | "thirdparty"
                    | "symbolic"
                    | "derived"
                    | "serde"
                    | "fixed_text"
            ) {
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

#[test]
fn structural_extent_dominance() {
    check_fixture("dominance");
}

#[test]
fn third_party_operation_summaries() {
    check_fixture("thirdparty");
}

#[test]
fn symbolic_generic_and_derived_costs() {
    check_fixture("symbolic");
}

#[test]
fn derived_call_costs() {
    check_fixture("derived");
}

#[test]
fn serde_derived_body_exclusion() {
    check_fixture("serde");
}

#[test]
fn fixed_text_value_proof() {
    check_fixture("fixed_text");
}
