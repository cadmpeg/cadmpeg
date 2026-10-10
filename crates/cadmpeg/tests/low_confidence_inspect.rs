// SPDX-License-Identifier: Apache-2.0
//! Single-codec agreement between inspect and load for a STEP archive root
//! outside the leading detection window.

#![cfg(all(feature = "step", not(any(feature = "fcstd", feature = "f3d"))))]

use std::fs;
use std::io::Write;

use assert_cmd::Command;
use tempfile::tempdir;

#[test]
fn delayed_step_archive_root_inspects_and_loads_with_step() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("delayed-root.zip");
    let file = fs::File::create(&input).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("preview.bin", options).unwrap();
    zip.write_all(&vec![0_u8; 140_000]).unwrap();
    zip.start_file("ISO-10303.p21", options).unwrap();
    zip.write_all(
        b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;ENDSEC;END-ISO-10303-21;",
    )
    .unwrap();
    zip.finish().unwrap();

    let inspected = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["inspect", input.to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(
        inspected.status.success(),
        "{}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let summary: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(summary["selection"]["kind"], "detected");
    // The leading window holds only a generic ZIP signal; the seekable
    // central directory then names the exact Part 21 root member.
    assert_eq!(summary["selection"]["confidence"], "medium");
    assert!(
        summary["summary"]["identity"]["dialects"]["primary"]["dialect"]
            .as_str()
            .is_some_and(|dialect| dialect.starts_with("step:"))
    );

    let loaded = Command::cargo_bin("cadmpeg")
        .unwrap()
        .args(["dump", input.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        loaded.status.success(),
        "{}",
        String::from_utf8_lossy(&loaded.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&loaded.stdout).unwrap();
    assert!(
        document["source"]["identity"]["dialects"]["primary"]["dialect"]
            .as_str()
            .is_some_and(|dialect| dialect.starts_with("step:"))
    );
}
