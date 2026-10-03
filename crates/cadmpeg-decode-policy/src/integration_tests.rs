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
    if let Ok(name) = std::env::var("CADMPEG_POLICY_CRATE_NAME") {
        args.extend(["--crate-name".to_owned(), name]);
    }
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
    if name == "work_keys" {
        command.env("CADMPEG_POLICY_CRATE_NAME", "cadmpeg_core");
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
            "stdout: {actual}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
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
                    | "reachability"
                    | "indirect"
                    | "addresses"
                    | "objects"
                    | "generic_scope"
                    | "fallback"
                    | "object_fallback"
                    | "method_scope"
                    | "pointer_scope"
                    | "path_scope"
                    | "coerced_addresses"
                    | "recursive_objects"
                    | "unresolved_objects"
                    | "generic_candidates"
                    | "symbolic_scope"
                    | "fixed_ranges"
                    | "raw_steps"
                    | "conversions"
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

fn check_graph_resolution(name: &str) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output_dir = root.join("target/fixtures").join(format!("{name}_graph"));
    std::fs::create_dir_all(&output_dir).expect("graph comparison directory");
    let run = |mode: &str| {
        Command::new(std::env::current_exe().expect("fixture executable"))
            .args([
                "--exact",
                "integration_tests::fixture_child",
                "--ignored",
                "--nocapture",
            ])
            .env("CADMPEG_POLICY_FIXTURE", "1")
            .env(mode, "1")
            .env(
                "CADMPEG_POLICY_INPUT",
                root.join("fixtures").join(format!("{name}.rs")),
            )
            .env("CADMPEG_POLICY_OUTPUT", &output_dir)
            .output()
            .expect("graph comparison compiler")
    };
    let graph = run("CADMPEG_POLICY_GRAPH");
    assert!(
        graph.status.success(),
        "{}",
        String::from_utf8_lossy(&graph.stderr)
    );
    let path = output_dir.join("graph.tsv");
    std::fs::write(&path, graph.stdout).expect("comparison graph rows");
    let joined = Command::new("python3")
        .args(["-c", "import runpy,sys; from pathlib import Path; m=runpy.run_path(sys.argv[1]); source=Path(sys.argv[2]).read_text(); reached,_=m['resolve_graph'](source); print('\\n'.join(m['unreachable_bodies'](source,reached)))"])
        .arg(root.join("../../scripts/check-decode-policy.py")).arg(path)
        .output().expect("joined graph resolver");
    assert!(
        joined.status.success(),
        "{}",
        String::from_utf8_lossy(&joined.stderr)
    );
    let local = run("CADMPEG_POLICY_UNREACHABLE");
    assert!(
        local.status.success(),
        "{}",
        String::from_utf8_lossy(&local.stderr)
    );
    let mut local: Vec<_> = String::from_utf8(local.stdout)
        .expect("local scope output")
        .lines()
        .filter(|line| line.starts_with("unreachable_decode_body\t"))
        .map(str::to_owned)
        .collect();
    local.sort();
    let joined: Vec<_> = String::from_utf8(joined.stdout)
        .expect("joined scope output")
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(local, joined, "{name}");
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
fn writer_without_decode_path() {
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

#[test]
fn decode_reachability() {
    check_fixture("reachability");
}

#[test]
fn constant_width_subslices() {
    check_fixture("fixed_ranges");
}

#[test]
fn charged_raw_steps() {
    check_fixture("raw_steps");
}

#[test]
fn charged_owned_conversions() {
    check_fixture("conversions");
}

#[test]
fn cross_crate_decode_reachability() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output_dir = root.join("target/fixtures/cross_scope");
    std::fs::create_dir_all(&output_dir).expect("cross-crate fixture directory");
    let executable = std::env::current_exe().expect("fixture compiler");
    let dependency = output_dir.join("libcadmpeg_core.rmeta");
    let run = |source: &str, name: &str, graph: bool, scope: Option<&std::path::Path>| {
        let mut command = Command::new(&executable);
        command
            .args([
                "--exact",
                "integration_tests::fixture_child",
                "--ignored",
                "--nocapture",
            ])
            .env("CADMPEG_POLICY_FIXTURE", "1")
            .env("CADMPEG_POLICY_INPUT", root.join("fixtures").join(source))
            .env("CADMPEG_POLICY_OUTPUT", &output_dir)
            .env("CADMPEG_POLICY_CRATE_NAME", name);
        if graph {
            command.env("CADMPEG_POLICY_GRAPH", "1");
        }
        if let Some(scope) = scope {
            command.env("CADMPEG_POLICY_SCOPE", scope);
        }
        if name != "cadmpeg_core" {
            command.env(
                "CADMPEG_POLICY_DEPENDENCY",
                format!("cadmpeg_core={}", dependency.display()),
            );
        }
        command.output().expect("cross-crate compiler")
    };
    let dependency_graph = run("reachability_dependency.rs", "cadmpeg_core", true, None);
    assert!(
        dependency_graph.status.success(),
        "{}",
        String::from_utf8_lossy(&dependency_graph.stderr)
    );
    let caller_graph = run(
        "reachability_imported.rs",
        "cadmpeg_codec_fixture",
        true,
        None,
    );
    assert!(
        caller_graph.status.success(),
        "{}",
        String::from_utf8_lossy(&caller_graph.stderr)
    );
    let graph = output_dir.join("graph.tsv");
    let scope = output_dir.join("scope.txt");
    std::fs::write(
        &graph,
        [dependency_graph.stdout, caller_graph.stdout].concat(),
    )
    .expect("graph rows");
    let resolved = Command::new("python3").args(["-c", "import runpy,sys; from pathlib import Path; module=runpy.run_path(sys.argv[1]); reached,_=module['resolve_graph'](Path(sys.argv[2]).read_text()); Path(sys.argv[3]).write_text(''.join(key+'\\n' for key in sorted(reached)))"])
        .arg(root.join("../../scripts/check-decode-policy.py")).arg(&graph).arg(&scope)
        .output().expect("global graph resolution");
    assert!(
        resolved.status.success(),
        "{}",
        String::from_utf8_lossy(&resolved.stderr)
    );
    let output = run(
        "reachability_dependency.rs",
        "cadmpeg_core",
        false,
        Some(&scope),
    );
    let findings: Vec<_> = String::from_utf8(output.stdout)
        .expect("cross-crate output")
        .lines()
        .filter(|line| line.starts_with("uncharged_decode_work\t"))
        .map(|line| line.split('\t').nth(2).expect("finding line").to_owned())
        .collect();
    let source = std::fs::read_to_string(root.join("fixtures/reachability_dependency.rs"))
        .expect("dependency source");
    let loop_lines: Vec<_> = source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("// reached-loop"))
        .map(|(index, line)| {
            if line.trim_start().starts_with("// reached-loop") {
                index.to_string()
            } else {
                (index + 1).to_string()
            }
        })
        .collect();
    let expected = loop_lines;
    assert_eq!(
        findings,
        expected,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(1));
    let caller = run(
        "reachability_imported.rs",
        "cadmpeg_codec_fixture",
        false,
        Some(&scope),
    );
    let actual = String::from_utf8(caller.stdout).expect("caller diagnostics");
    let source = std::fs::read_to_string(root.join("fixtures/reachability_imported.rs"))
        .expect("caller source");
    let loops: Vec<_> = source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("// reached-loop"))
        .map(|(index, line)| {
            if line.trim_start().starts_with("// reached-loop") {
                index.to_string()
            } else {
                (index + 1).to_string()
            }
        })
        .collect();
    let findings: Vec<_> = actual
        .lines()
        .filter(|line| line.starts_with("uncharged_decode_work\t"))
        .map(|line| {
            line.split('\t')
                .nth(2)
                .expect("caller finding line")
                .to_owned()
        })
        .collect();
    assert_eq!(
        findings,
        loops,
        "{actual}; {}",
        String::from_utf8_lossy(&caller.stderr)
    );
    assert_eq!(caller.status.code(), Some(1));
}

#[test]
fn static_pointer_and_object_reachability() {
    check_fixture("indirect");
}

#[test]
fn address_taken_reachability() {
    check_fixture("addresses");
}

#[test]
fn object_coercion_reachability() {
    check_fixture("objects");
}

#[test]
fn concrete_generic_reachability() {
    check_fixture("generic_scope");
}

#[test]
fn unresolved_indirect_fallback() {
    check_fixture("fallback");
}

#[test]
fn excluded_body_listing() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output_dir = root.join("target/fixtures/listing");
    std::fs::create_dir_all(&output_dir).expect("listing directory");
    let output = Command::new(std::env::current_exe().expect("fixture executable"))
        .args([
            "--exact",
            "integration_tests::fixture_child",
            "--ignored",
            "--nocapture",
        ])
        .env("CADMPEG_POLICY_FIXTURE", "1")
        .env("CADMPEG_POLICY_UNREACHABLE", "1")
        .env(
            "CADMPEG_POLICY_INPUT",
            root.join("fixtures/reachability.rs"),
        )
        .env("CADMPEG_POLICY_OUTPUT", output_dir)
        .output()
        .expect("listing compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual = String::from_utf8(output.stdout).expect("listing UTF-8");
    let rows: Vec<_> = actual
        .lines()
        .filter(|line| line.starts_with("unreachable_decode_body\t"))
        .collect();
    assert_eq!(rows.len(), 3, "{actual}");
    for (name, reason) in [
        ("<Backend as CodecBackend>::encode", "encoder-only"),
        ("encoder_helper", "no path from a decode entry point"),
        ("unrelated", "no path from a decode entry point"),
    ] {
        assert!(
            rows.iter().any(|row| {
                let fields: Vec<_> = row.split('\t').collect();
                fields.len() == 5
                    && fields[1].ends_with("fixtures/reachability.rs")
                    && fields[2].parse::<usize>().is_ok()
                    && fields[3].ends_with(name)
                    && fields[4] == reason
            }),
            "missing {name}: {actual}"
        );
    }
}

#[test]
fn lifetime_object_fallback() {
    check_fixture("object_fallback");
}

#[test]
fn unconstrained_generic_root_reachability() {
    check_fixture("symbolic_scope");
}

#[test]
fn called_object_methods_only() {
    check_fixture("method_scope");
    check_graph_resolution("method_scope");
}

#[test]
fn type_compatible_indirect_candidates() {
    check_fixture("pointer_scope");
    check_graph_resolution("pointer_scope");
}

#[test]
fn shortest_decode_paths() {
    check_fixture("path_scope");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output_dir = root.join("target/fixtures/path_graph");
    std::fs::create_dir_all(&output_dir).expect("path graph directory");
    let output = Command::new(std::env::current_exe().expect("fixture executable"))
        .args([
            "--exact",
            "integration_tests::fixture_child",
            "--ignored",
            "--nocapture",
        ])
        .env("CADMPEG_POLICY_FIXTURE", "1")
        .env("CADMPEG_POLICY_GRAPH", "1")
        .env("CADMPEG_POLICY_INPUT", root.join("fixtures/path_scope.rs"))
        .env("CADMPEG_POLICY_OUTPUT", &output_dir)
        .output()
        .expect("path graph compiler");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let graph = output_dir.join("graph.tsv");
    std::fs::write(&graph, output.stdout).expect("path graph rows");
    let explain = |name: &str| {
        let output = Command::new("python3")
            .arg(root.join("../../scripts/check-decode-policy.py"))
            .arg("--graph-input")
            .arg(&graph)
            .arg("--explain-body")
            .arg(name)
            .output()
            .expect("path mode");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("path UTF-8")
    };
    let direct = explain("leaf");
    assert_eq!(
        direct
            .lines()
            .filter(|line| line.starts_with("decode_path_edge\t"))
            .count(),
        2,
        "{direct}"
    );
    assert!(direct.contains("direct call"), "{direct}");
    assert!(direct.contains("short ("), "{direct}");
    assert!(!direct.contains("long ("), "{direct}");
    let address = explain("addressed");
    assert!(address.contains("function address"), "{address}");
    let fallback = explain("fallback");
    assert!(
        fallback.contains("unresolved-indirect candidate"),
        "{fallback}"
    );
    let object = explain("<Inner as Work>::work");
    assert!(object.contains("trait-object call"), "{object}");
    assert!(object.contains("generic instantiation"), "{object}");
    let excluded = explain("encode");
    assert!(excluded.ends_with("\tunreachable\n"), "{excluded}");
    let source =
        std::fs::read_to_string(root.join("fixtures/path_scope.rs")).expect("path fixture source");
    let line = source
        .lines()
        .position(|line| line.contains("for byte in bytes"))
        .expect("leaf loop")
        + 1;
    assert_eq!(direct, explain(&format!("fixtures/path_scope.rs:{line}")));
}

#[test]
fn coerced_function_and_method_addresses() {
    check_fixture("coerced_addresses");
}

#[test]
fn recursive_object_instances() {
    check_fixture("recursive_objects");
}

#[test]
fn unresolved_object_generic_and_closure_candidates() {
    check_fixture("unresolved_objects");
}

#[test]
fn generic_candidate_substitutions() {
    check_fixture("generic_candidates");
    check_graph_resolution("generic_candidates");
}

#[test]
fn admitted_iteration_work() {
    check_fixture("work_admitted");
}

#[test]
fn complete_key_work_receipts() {
    check_fixture("work_keys");
}
