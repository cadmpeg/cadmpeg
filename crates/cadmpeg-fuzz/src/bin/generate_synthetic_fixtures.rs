// SPDX-License-Identifier: Apache-2.0
//! Generates deterministic CadIr JSON fixtures for IR and STEP fuzz targets.

use std::fs;
use std::path::{Path, PathBuf};

use cadmpeg_ir::CadIr;

const JSON_TARGETS: [&str; 5] = [
    "ir_from_json",
    "ir_validate",
    "ir_canonical_roundtrip",
    "step_writer",
    "f3d_writer",
];

fn main() {
    let minimal = CadIr::empty(Default::default())
        .to_canonical_json()
        .expect("serialize minimal CadIr");
    let unit_cube = cadmpeg_ir::examples::unit_cube()
        .to_canonical_json()
        .expect("serialize unit-cube CadIr");
    let directed_subd_sum = cadmpeg_ir::examples::directed_subd_sum()
        .to_canonical_json()
        .expect("serialize directed SubD and Sum CadIr");
    let documents = [
        ("minimal_v13.json", minimal.as_bytes()),
        ("unit_cube_v13.json", unit_cube.as_bytes()),
        ("directed_subd_sum_v13.json", directed_subd_sum.as_bytes()),
    ];
    let valid_v0 = minimal.replacen(r#""ir_version": "54""#, r#""ir_version": "0""#, 1);
    for (_, document) in documents {
        CadIr::from_json(std::str::from_utf8(document).expect("fixture is UTF-8"))
            .expect("fixture is valid current-version CadIr");
    }
    assert!(
        CadIr::from_json(&valid_v0).is_err(),
        "CadIr v0 fixture must be rejected"
    );

    for target in JSON_TARGETS {
        let directory = seed_directory(target);
        replace_directory_contents(&directory);
        for (name, contents) in documents {
            write(&directory, name, contents);
        }
    }

    let migration_directory = seed_directory("ir_migrate_json");
    replace_directory_contents(&migration_directory);
    for (name, contents) in documents {
        let legacy = std::str::from_utf8(contents)
            .expect("fixture is UTF-8")
            .replacen(r#""ir_version": "54""#, r#""ir_version": "53""#, 1);
        let name = name.replace("_v13.json", "_v12.json");
        write(&migration_directory, &name, legacy.as_bytes());
    }

    write(
        &seed_directory("ir_from_json"),
        "valid_v0_rejected.json",
        valid_v0.as_bytes(),
    );

    generate_diff_seeds(&minimal, &unit_cube);
    generate_prefixed_seeds("ir_validate_mutated", 1, &documents);
    generate_prefixed_seeds("step_writer_custom", 8, &documents);
}

fn seed_directory(target: &str) -> PathBuf {
    Path::new("seeds").join(target)
}

fn replace_directory_contents(directory: &Path) {
    fs::create_dir_all(directory).expect("create seed directory");
    for entry in fs::read_dir(directory).expect("read seed directory") {
        let path = entry.expect("read seed entry").path();
        if path.is_dir() {
            fs::remove_dir_all(path).expect("remove stale seed directory");
        } else {
            fs::remove_file(path).expect("remove stale seed");
        }
    }
}

fn write(directory: &Path, name: &str, contents: &[u8]) {
    let path = directory.join(name);
    fs::write(&path, contents).expect("write seed");
    println!("wrote {} ({} bytes)", path.display(), contents.len());
}

fn generate_diff_seeds(minimal: &str, unit_cube: &str) {
    let directory = seed_directory("ir_diff");
    replace_directory_contents(&directory);
    for (name, selector, left, right) in [
        ("minimal_vs_minimal", 0_u8, minimal, minimal),
        ("minimal_vs_cube", 1_u8, minimal, unit_cube),
        ("cube_vs_minimal", 2_u8, unit_cube, minimal),
    ] {
        let mut contents = Vec::with_capacity(2 + left.len() + right.len());
        contents.push(selector);
        contents.extend_from_slice(left.as_bytes());
        contents.push(0);
        contents.extend_from_slice(right.as_bytes());
        write(&directory, name, &contents);
    }
}

fn generate_prefixed_seeds(target: &str, prefix_length: usize, documents: &[(&str, &[u8])]) {
    let directory = seed_directory(target);
    replace_directory_contents(&directory);
    for (index, (name, document)) in documents.iter().enumerate() {
        let mut contents = vec![index as u8; prefix_length];
        contents.extend_from_slice(document);
        write(&directory, name, &contents);
    }
}
