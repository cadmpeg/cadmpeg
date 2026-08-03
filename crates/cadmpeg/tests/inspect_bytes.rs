// SPDX-License-Identifier: Apache-2.0
//! Integration tests for the `cadmpeg inspect` byte subcommands.
//!
//! Every fixture is built here from literal bytes, so each expected value is
//! derived from the construction rather than read back from a run.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

/// Writes `bytes` to `name` inside `dir` and returns the path.
fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

/// A 36-byte record: a 4-byte tag, a little-endian count, four padding bytes,
/// then two big-endian binary64 values and an 8-byte ASCII name.
fn record_fixture() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RECS");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0xaa; 4]);
    // 1.5 is 0x3ff8_0000_0000_0000; -0.5 is 0xbfe0_0000_0000_0000.
    bytes.extend_from_slice(&0x3ff8_0000_0000_0000u64.to_be_bytes());
    bytes.extend_from_slice(&0xbfe0_0000_0000_0000u64.to_be_bytes());
    bytes.extend_from_slice(b"widget00");
    assert_eq!(bytes.len(), 4 + 4 + 4 + 8 + 8 + 8);
    bytes
}

fn cadmpeg() -> Command {
    Command::cargo_bin("cadmpeg").unwrap()
}

#[test]
fn hex_dumps_an_absolute_window_and_accepts_hex_arguments() {
    let dir = tempdir().unwrap();
    let bytes: Vec<u8> = (0u8..64).collect();
    let file = write(dir.path(), "counter.bin", &bytes);

    // Offsets 0x10..0x20 hold the bytes 16..32.
    cadmpeg()
        .args([
            "inspect",
            "hex",
            file.to_str().unwrap(),
            "--offset",
            "0x10",
            "--len",
            "0x10",
        ])
        .assert()
        .success()
        .stdout(
            "00000010  10 11 12 13 14 15 16 17  18 19 1a 1b 1c 1d 1e 1f  \
             |................|\n",
        );

    // The same window written in decimal must produce the same dump.
    cadmpeg()
        .args([
            "inspect",
            "hex",
            file.to_str().unwrap(),
            "--offset",
            "16",
            "--len",
            "16",
        ])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("00000010  10 11 12 13"));
}

#[test]
fn hex_stops_at_end_of_file_and_rejects_an_offset_past_it() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "short.bin", b"abc");

    cadmpeg()
        .args(["inspect", "hex", file.to_str().unwrap(), "--len", "0x100"])
        .assert()
        .success()
        .stdout(predicate::str::contains("|abc|"));

    cadmpeg()
        .args(["inspect", "hex", file.to_str().unwrap(), "--offset", "4"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("past the end"));
}

#[test]
fn read_walks_a_record_array_with_an_explicit_stride() {
    let dir = tempdir().unwrap();
    // Three u16be counts at offsets 0, 6 and 12, with four filler bytes between.
    let mut bytes = Vec::new();
    for count in [7u16, 8, 9] {
        bytes.extend_from_slice(&count.to_be_bytes());
        bytes.extend_from_slice(&[0xff; 4]);
    }
    let file = write(dir.path(), "array.bin", &bytes);

    cadmpeg()
        .args([
            "inspect",
            "read",
            file.to_str().unwrap(),
            "--type",
            "u16",
            "--be",
            "--count",
            "3",
            "--stride",
            "6",
        ])
        .assert()
        .success()
        .stdout(
            "0x00000000  u16be   7                         0x0007\n\
             0x00000006  u16be   8                         0x0008\n\
             0x0000000c  u16be   9                         0x0009\n",
        );
}

#[test]
fn read_defaults_to_little_endian_and_reports_a_read_past_the_end() {
    let dir = tempdir().unwrap();
    // 0x0102 little-endian is 513; the same bytes big-endian are 258.
    let file = write(dir.path(), "pair.bin", &[0x01, 0x02]);

    cadmpeg()
        .args(["inspect", "read", file.to_str().unwrap(), "--type", "u16"])
        .assert()
        .success()
        .stdout(predicate::str::contains("513"));

    cadmpeg()
        .args(["inspect", "read", file.to_str().unwrap(), "--type", "u32"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("past the end"));
}

#[test]
fn find_reports_pattern_string_and_wildcard_hits() {
    let dir = tempdir().unwrap();
    let mut bytes = b"prefix".to_vec();
    bytes.extend_from_slice(&[0x4d, 0x5a, 0x01, 0x00]);
    bytes.extend_from_slice(&[0x4d, 0x5a, 0x02, 0x00]);
    for unit in "Part".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    // "prefix" is 6 bytes, so the two 4-byte stubs sit at 6 and 10 and the
    // UTF-16LE text starts at 14.
    let file = write(dir.path(), "hits.bin", &bytes);

    cadmpeg()
        .args([
            "inspect",
            "find",
            file.to_str().unwrap(),
            "--hex",
            "4d5a??00",
        ])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("hits: 2")
                .and(predicate::str::contains("0x00000006"))
                .and(predicate::str::contains("0x0000000a")),
        );

    cadmpeg()
        .args([
            "inspect",
            "find",
            file.to_str().unwrap(),
            "--ascii",
            "prefix",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("0x00000000"));

    cadmpeg()
        .args([
            "inspect",
            "find",
            file.to_str().unwrap(),
            "--utf16le",
            "Part",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("hits: 1").and(predicate::str::contains("0x0000000e")));
}

#[test]
fn find_rejects_a_positional_pattern_and_names_the_three_flags() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "hits.bin", b"prefix");

    // A bare word does not say how to encode it, so the positional form stays
    // an error. The error names the flag that carries each encoding.
    cadmpeg()
        .args(["inspect", "find", file.to_str().unwrap(), "prefix"])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("--hex prefix")
                .and(predicate::str::contains("--ascii prefix"))
                .and(predicate::str::contains("--utf16le prefix")),
        );
}

#[test]
fn find_notes_truncation_after_the_last_hit() {
    let dir = tempdir().unwrap();
    // Four single-byte hits at offsets 0..4, of which `--max 2` reports two.
    let file = write(dir.path(), "many.bin", b"aaaa");

    cadmpeg()
        .args([
            "inspect",
            "find",
            file.to_str().unwrap(),
            "--ascii",
            "a",
            "--max",
            "2",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "note: output truncated at 2 matches; pass --max 0 for all",
        ));

    // Every hit fits under the default limit, so no note is printed.
    cadmpeg()
        .args(["inspect", "find", file.to_str().unwrap(), "--ascii", "a"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("hits: 4").and(predicate::str::contains("truncated").not()),
        );
}

#[test]
fn guessed_flag_spellings_reach_the_same_arguments() {
    let dir = tempdir().unwrap();
    let bytes: Vec<u8> = (0u8..64).collect();
    let counter = write(dir.path(), "counter.bin", &bytes);
    let text = write(dir.path(), "text.bin", b"\x00document\x00");

    // `--length` and `--start` are aliases of `--len` and `--offset`.
    let expected = "00000010  10 11 12 13 14 15 16 17  18 19 1a 1b 1c 1d 1e 1f  \
                    |................|\n";
    for flags in [["--offset", "--len"], ["--start", "--length"]] {
        cadmpeg()
            .args([
                "inspect",
                "hex",
                counter.to_str().unwrap(),
                flags[0],
                "0x10",
                flags[1],
                "0x10",
            ])
            .assert()
            .success()
            .stdout(expected);
    }

    // `--min-len` and `--min-length` are aliases of `--min`.
    for flag in ["--min", "--min-len", "--min-length"] {
        cadmpeg()
            .args(["inspect", "strings", text.to_str().unwrap(), flag, "8"])
            .assert()
            .success()
            .stdout("0x00000001  ascii     \"document\"\n");
    }

    // `-n` is the short form of `--count` on both `read` and `struct`.
    cadmpeg()
        .args([
            "inspect",
            "read",
            counter.to_str().unwrap(),
            "--type",
            "u8",
            "-n",
            "2",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("0x00000000").and(predicate::str::contains("0x00000001")));
    cadmpeg()
        .args([
            "inspect",
            "struct",
            counter.to_str().unwrap(),
            "--layout",
            "u8:byte",
            "-n",
            "2",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("record 0").and(predicate::str::contains("record 1")));
}

#[test]
fn the_bytes_group_prefix_reaches_the_tool_and_its_help() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "counter.bin", &(0u8..16).collect::<Vec<u8>>());

    // `inspect bytes hex --help` prints the tool's help rather than a
    // subcommand-conflict error. The usage line carries both path segments and
    // the arguments belong to `hex`. The predicate omits the leading executable
    // name, which clap takes from `argv[0]` and renders as `cadmpeg.exe` on
    // Windows.
    cadmpeg()
        .args(["inspect", "bytes", "hex", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("inspect bytes hex [OPTIONS] <FILE>")
                .and(predicate::str::contains("--width <WIDTH>")),
        );

    // The grouped form runs the same tool as the direct form.
    let expected = "00000000  00 01 02 03 04 05 06 07  08 09 0a 0b 0c 0d 0e 0f  \
                    |................|\n";
    cadmpeg()
        .args(["inspect", "bytes", "hex", file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(expected);
    cadmpeg()
        .args(["inspect", "hex", file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(expected);
}

#[test]
fn find_rejects_a_malformed_pattern_and_a_missing_needle() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "empty.bin", b"");

    cadmpeg()
        .args(["inspect", "find", file.to_str().unwrap(), "--hex", "4d5"])
        .assert()
        .code(2);

    cadmpeg()
        .args(["inspect", "find", file.to_str().unwrap()])
        .assert()
        .failure();
}

#[test]
fn strings_honours_the_minimum_length_and_the_encoding() {
    let dir = tempdir().unwrap();
    let mut bytes = vec![0x00];
    bytes.extend_from_slice(b"body");
    bytes.push(0x00);
    bytes.extend_from_slice(b"no");
    bytes.push(0xff);
    for unit in "sketch".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    // "body" is at 1; "no" is at 6 and is below the minimum; the UTF-16LE run
    // starts at 9.
    let file = write(dir.path(), "text.bin", &bytes);

    cadmpeg()
        .args(["inspect", "strings", file.to_str().unwrap(), "--min", "4"])
        .assert()
        .success()
        .stdout("0x00000001  ascii     \"body\"\n");

    cadmpeg()
        .args([
            "inspect",
            "strings",
            file.to_str().unwrap(),
            "--min",
            "4",
            "--encoding",
            "utf16le",
        ])
        .assert()
        .success()
        .stdout("0x00000009  utf16le   \"sketch\"\n");
}

#[test]
fn struct_decodes_consecutive_records() {
    let dir = tempdir().unwrap();
    let mut bytes = record_fixture();
    bytes.extend_from_slice(&record_fixture());
    let file = write(dir.path(), "records.bin", &bytes);

    cadmpeg()
        .args([
            "inspect",
            "struct",
            file.to_str().unwrap(),
            "--layout",
            "bytes4:tag,u32le:count,pad4,f64be:x,f64be:y,bytes8:name",
            "--count",
            "2",
        ])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("record 0 @ 0x00000000 (36 bytes)")
                .and(predicate::str::contains("record 1 @ 0x00000024 (36 bytes)"))
                .and(predicate::str::contains("52 45 43 53"))
                .and(predicate::str::contains("0x3ff8000000000000"))
                .and(predicate::str::contains("-0.5"))
                .and(predicate::str::contains("count  u32le     2")),
        );
}

#[test]
fn struct_rejects_a_bad_layout_and_a_run_past_the_end() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "records.bin", &record_fixture());

    cadmpeg()
        .args([
            "inspect",
            "struct",
            file.to_str().unwrap(),
            "--layout",
            "u32:count",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("byte-order suffix"));

    cadmpeg()
        .args([
            "inspect",
            "struct",
            file.to_str().unwrap(),
            "--layout",
            "u32le:count",
            "--count",
            "100",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("bytes"));
}

#[test]
fn container_lists_zip_entries_with_shell_safe_names() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("model.f3d");
    let file = fs::File::create(&path).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive
        .start_file("FusionAssetName[Active]/Design.dat", stored)
        .unwrap();
    archive.write_all(b"payload").unwrap();
    archive.finish().unwrap();

    cadmpeg()
        .args(["inspect", "container", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("'FusionAssetName[Active]/Design.dat'")
                .and(predicate::str::contains("stored"))
                // "payload" is 7 stored bytes, so packed and unpacked agree.
                .and(predicate::str::contains("7             7")),
        );
}

#[test]
fn container_refuses_a_file_that_is_not_a_zip() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "plain.bin", b"not a zip archive at all");

    cadmpeg()
        .args(["inspect", "container", file.to_str().unwrap()])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("as a ZIP container"));
}

#[test]
fn diff_reports_identity_length_and_the_first_difference() {
    let dir = tempdir().unwrap();
    let base: Vec<u8> = (0u8..32).collect();
    let same = write(dir.path(), "same.bin", &base);
    let copy = write(dir.path(), "copy.bin", &base);

    cadmpeg()
        .args([
            "inspect",
            "diff",
            same.to_str().unwrap(),
            copy.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("identical"));

    // Change byte 5 and byte 20, then append two bytes.
    let mut variant = base.clone();
    variant[5] = 0xff;
    variant[20] = 0xff;
    variant.extend_from_slice(&[0xee, 0xee]);
    let other = write(dir.path(), "variant.bin", &variant);

    cadmpeg()
        .args([
            "inspect",
            "diff",
            same.to_str().unwrap(),
            other.to_str().unwrap(),
            "--gap",
            "0",
        ])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("first difference: 0x00000005 (5)")
                .and(predicate::str::contains("differing bytes: 2 of 32"))
                .and(predicate::str::contains("length differs by 2 bytes"))
                .and(predicate::str::contains("0x00000005..0x00000006"))
                .and(predicate::str::contains("0x00000014..0x00000015")),
        );
}

#[test]
fn diff_coalesces_runs_at_the_requested_gap() {
    let dir = tempdir().unwrap();
    let base = vec![0u8; 16];
    let mut variant = base.clone();
    // Differ at 2 and 6, leaving three equal bytes between the two spans.
    variant[2] = 1;
    variant[6] = 1;
    let a = write(dir.path(), "a.bin", &base);
    let b = write(dir.path(), "b.bin", &variant);

    cadmpeg()
        .args([
            "inspect",
            "diff",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--gap",
            "3",
        ])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("runs (gap 3): 1")
                .and(predicate::str::contains("0x00000002..0x00000007  5 bytes")),
        );
}

#[test]
fn inspect_without_a_subcommand_still_runs_the_container_summary() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("document.FCStd");
    let file = fs::File::create(&path).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file(
            "Document.xml",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
    archive
        .write_all(
            b"<Document SchemaVersion=\"4\" FileVersion=\"1\" ProgramVersion=\"1.0\">\
              <Object/></Document>",
        )
        .unwrap();
    archive.finish().unwrap();

    cadmpeg()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("format: fcstd (detected high)"));
}

#[test]
fn inspect_without_an_input_or_a_subcommand_is_a_usage_error() {
    cadmpeg()
        .arg("inspect")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage").and(predicate::str::contains("INPUT")));
}

#[test]
fn inspect_help_lists_every_byte_subcommand() {
    let mut expected = predicate::str::contains("hex").boxed();
    for name in ["read", "find", "strings", "struct", "container", "diff"] {
        expected = expected.and(predicate::str::contains(name)).boxed();
    }
    cadmpeg()
        .args(["inspect", "--help"])
        .assert()
        .success()
        .stdout(expected);
}
