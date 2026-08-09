// SPDX-License-Identifier: Apache-2.0
//! External openNURBS transfer witness.

use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};

use super::RhinoCodec;

const BASELINE: [(u64, usize, usize); 7] = [
    (2, 1989, 2342),
    (3, 2413, 2477),
    (4, 47, 173),
    (50, 92, 198),
    (60, 28, 37),
    (70, 31, 46),
    (80, 24, 39),
];

fn files(root: &Path, output: &mut Vec<PathBuf>) {
    let mut entries = fs::read_dir(root)
        .expect("read openNURBS example directory")
        .map(|entry| entry.expect("read openNURBS example entry").path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "3dm") {
            output.push(path);
        }
    }
}

fn note_count(notes: &[String]) -> Option<(usize, usize)> {
    notes.iter().find_map(|note| {
        let rest = note.strip_prefix("decoded ")?;
        let fraction = rest.split_whitespace().next()?;
        let (decoded, total) = fraction.split_once('/')?;
        Some((decoded.parse().ok()?, total.parse().ok()?))
    })
}

fn archive_version(notes: &[String]) -> Option<u64> {
    notes
        .iter()
        .find_map(|note| note.strip_prefix("archive version ")?.parse().ok())
}

fn decode_counts(path: &Path) -> Option<(u64, usize, usize)> {
    let bytes = fs::read(path).expect("read 3DM witness");
    let inspect = RhinoCodec
        .inspect(&mut Cursor::new(bytes.clone()), &InspectOptions::default())
        .expect("inspect witness");
    let version = archive_version(&inspect.notes).expect("archive version note");
    let decoded = RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .ok()?;
    let (supported, total) = note_count(&decoded.report.notes).unwrap_or((0, 0));
    if supported < total && std::env::var_os("RHINO_WITNESS_DIAGNOSTICS").is_some() {
        eprintln!("{}: {supported}/{total}", path.display());
        for loss in &decoded.report.losses {
            eprintln!("  {}: {}", loss.code, loss.message);
        }
    }
    let validation = cadmpeg_ir::validate(&decoded.ir, Vec::new());
    assert!(
        validation.findings.iter().all(|finding| !matches!(
            finding.severity,
            cadmpeg_ir::report::Severity::Error | cadmpeg_ir::report::Severity::Blocking
        )),
        "validation failed for {}",
        path.display()
    );
    Some((version, supported, total))
}

fn oracle_object_count(output: &[u8]) -> usize {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|line| {
            let suffix = line.trim().strip_prefix("ModelGeometry ")?;
            suffix.strip_suffix(':')?.parse::<usize>().ok()
        })
        .max()
        .map_or(0, |index| index + 1)
}

#[test]
#[ignore = "requires OPENNURBS_ROOT and an openNURBS example_read executable"]
fn opennurbs_object_walk_and_transfer_floor() {
    let root = PathBuf::from(std::env::var_os("OPENNURBS_ROOT").expect("OPENNURBS_ROOT"));
    let reader = root.join("example_read/example_read");
    assert!(reader.is_file(), "build openNURBS example_read first");
    let mut inputs = Vec::new();
    files(&root.join("example_files"), &mut inputs);
    assert_eq!(inputs.len(), 153, "unexpected openNURBS example corpus");

    let mut counts = BTreeMap::<u64, (usize, usize)>::new();
    for path in inputs {
        let witness = Command::new(&reader)
            .arg(&path)
            .output()
            .expect("run openNURBS example_read");
        assert!(
            witness.status.success(),
            "example_read refused {}",
            path.display()
        );
        let oracle_total = oracle_object_count(&witness.stdout);

        let Some((version, supported, total)) = decode_counts(&path) else {
            continue;
        };
        if total > 0 {
            assert_eq!(
                total,
                oracle_total,
                "object walk differs for {}",
                path.display()
            );
        }
        let entry = counts.entry(version).or_default();
        entry.0 += supported;
        entry.1 += total;
    }

    if std::env::var_os("RHINO_WITNESS_DIAGNOSTICS").is_some() {
        for (version, actual) in &counts {
            eprintln!("archive {version}: {}/{}", actual.0, actual.1);
        }
    }
    for (version, minimum_supported, expected_total) in BASELINE {
        let actual = counts.get(&version).copied().unwrap_or_default();
        assert_eq!(
            actual.1, expected_total,
            "archive {version} object-walk drift"
        );
        assert!(
            actual.0 >= minimum_supported,
            "archive {version} transfer regressed: {} < {minimum_supported}",
            actual.0
        );
    }

    let generated =
        PathBuf::from(std::env::var_os("OPENNURBS_SYNTH_DIR").expect("OPENNURBS_SYNTH_DIR"));
    for version in [50, 60, 70, 80] {
        let path = generated.join(format!("witness-v{version}.3dm"));
        let witness = Command::new(&reader)
            .arg(&path)
            .output()
            .expect("run example_read on synthesized witness");
        assert!(
            witness.status.success(),
            "example_read refused {}",
            path.display()
        );
        assert_eq!(decode_counts(&path), Some((version, 1, 1)));
    }
}
