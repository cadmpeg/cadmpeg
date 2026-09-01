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

    cadmpeg()
        .args(["inspect", "bytes", "hex", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("inspect bytes hex [OPTIONS] [FILE]")
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
        .stderr(predicate::str::contains("as a ZIP or CFB container"));
}

#[test]
fn cmp_reports_identity_length_and_the_first_difference() {
    let dir = tempdir().unwrap();
    let base: Vec<u8> = (0u8..32).collect();
    let same = write(dir.path(), "same.bin", &base);
    let copy = write(dir.path(), "copy.bin", &base);

    cadmpeg()
        .args([
            "inspect",
            "cmp",
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
            "cmp",
            same.to_str().unwrap(),
            other.to_str().unwrap(),
            "--gap",
            "0",
        ])
        .assert()
        .code(1)
        .stdout(
            predicate::str::contains("first difference: 0x00000005 (5)")
                .and(predicate::str::contains("differing bytes: 2 of 32"))
                .and(predicate::str::contains("length differs by 2 bytes"))
                .and(predicate::str::contains("0x00000005..0x00000006"))
                .and(predicate::str::contains("0x00000014..0x00000015")),
        );
}

#[test]
fn cmp_coalesces_runs_at_the_requested_gap() {
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
            "cmp",
            a.to_str().unwrap(),
            b.to_str().unwrap(),
            "--gap",
            "3",
        ])
        .assert()
        .code(1)
        .stdout(
            predicate::str::contains("runs (gap 3): 1")
                .and(predicate::str::contains("0x00000002..0x00000007  5 bytes")),
        );
}

#[test]
fn cmp_exits_one_when_only_lengths_differ() {
    let dir = tempdir().unwrap();
    let prefix = b"shared-prefix";
    let shorter = write(dir.path(), "short.bin", prefix);
    let mut longer_bytes = prefix.to_vec();
    longer_bytes.extend_from_slice(b"extra");
    let longer = write(dir.path(), "long.bin", &longer_bytes);

    cadmpeg()
        .args([
            "inspect",
            "cmp",
            shorter.to_str().unwrap(),
            longer.to_str().unwrap(),
        ])
        .assert()
        .code(1)
        .stdout(
            predicate::str::contains("length differs")
                .and(predicate::str::contains("the common prefix is identical")),
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
    for name in ["read", "find", "strings", "struct", "container", "cmp"] {
        expected = expected.and(predicate::str::contains(name)).boxed();
    }
    cadmpeg()
        .args(["inspect", "--help"])
        .assert()
        .success()
        .stdout(expected);
}

/// Builds a ZIP with a bracketed Fusion-style name and a deflated member.
fn extract_fixture(dir: &Path) -> PathBuf {
    let path = dir.join("model.f3d");
    let file = fs::File::create(&path).unwrap();
    let mut archive = zip::ZipWriter::new(file);
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("Body[Active].brp", stored).unwrap();
    archive.write_all(b"bracketed payload").unwrap();
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    archive.start_file("Design/Streams.dat", deflated).unwrap();
    archive.write_all(&[0x42u8; 512]).unwrap();
    archive.finish().unwrap();
    path
}

const CFB_SECTOR: usize = 512;
const CFB_FREE: u32 = 0xffff_ffff;
const CFB_END: u32 = 0xffff_fffe;
const CFB_FAT: u32 = 0xffff_fffd;

/// One FAT sector, eight data sectors of 0x5a, and a directory with Root Entry
/// plus a 4096-byte Payload stream. Same construction as the inspect container
/// unit fixture.
fn compound_fixture() -> Vec<u8> {
    let mut file = vec![0_u8; CFB_SECTOR * 11];
    file[..8].copy_from_slice(&[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    put_u16(&mut file, 24, 0x003e);
    put_u16(&mut file, 26, 3);
    put_u16(&mut file, 28, 0xfffe);
    put_u16(&mut file, 30, 9);
    put_u16(&mut file, 32, 6);
    put_u32(&mut file, 44, 1);
    put_u32(&mut file, 48, 0);
    put_u32(&mut file, 56, 4096);
    put_u32(&mut file, 60, CFB_END);
    put_u32(&mut file, 68, CFB_END);
    for index in 0..109 {
        put_u32(&mut file, 76 + index * 4, CFB_FREE);
    }
    put_u32(&mut file, 76, 9);
    let directory = sector_mut(&mut file, 0);
    for entry in directory.chunks_exact_mut(128) {
        entry[68..80].fill(0xff);
    }
    directory_entry(directory, 0, "Root Entry", 5, 1, CFB_END, 0);
    directory_entry(directory, 1, "Payload", 2, CFB_FREE, 1, 4096);
    for sector in 1..=8 {
        sector_mut(&mut file, sector).fill(0x5a);
    }
    let fat = sector_mut(&mut file, 9);
    fat.fill(0xff);
    put_u32(fat, 0, CFB_END);
    for sector in 1..8 {
        put_u32(fat, sector * 4, (sector + 1) as u32);
    }
    put_u32(fat, 8 * 4, CFB_END);
    put_u32(fat, 9 * 4, CFB_FAT);
    file
}

fn directory_entry(
    directory: &mut [u8],
    index: usize,
    name: &str,
    object_type: u8,
    child: u32,
    start: u32,
    size: u64,
) {
    let entry = &mut directory[index * 128..(index + 1) * 128];
    let units = name.encode_utf16().collect::<Vec<_>>();
    for (offset, unit) in units.iter().enumerate() {
        put_u16(entry, offset * 2, *unit);
    }
    put_u16(entry, 64, ((units.len() + 1) * 2) as u16);
    entry[66] = object_type;
    entry[67] = 1;
    put_u32(entry, 68, CFB_FREE);
    put_u32(entry, 72, CFB_FREE);
    put_u32(entry, 76, child);
    put_u32(entry, 116, start);
    entry[120..128].copy_from_slice(&size.to_le_bytes());
}

fn sector_mut(file: &mut [u8], sector: usize) -> &mut [u8] {
    let start = (sector + 1) * CFB_SECTOR;
    &mut file[start..start + CFB_SECTOR]
}

fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn extract_writes_a_member_and_streams_it_to_stdout() {
    let dir = tempdir().unwrap();
    let archive = extract_fixture(dir.path());
    let out = dir.path().join("streams.dat");

    cadmpeg()
        .args([
            "inspect",
            "extract",
            archive.to_str().unwrap(),
            "Design/Streams.dat",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(fs::read(&out).unwrap(), vec![0x42u8; 512]);

    let stdout = cadmpeg()
        .args([
            "inspect",
            "extract",
            archive.to_str().unwrap(),
            "Design/Streams.dat",
        ])
        .output()
        .unwrap();
    assert!(stdout.status.success());
    assert_eq!(stdout.stdout, fs::read(&out).unwrap());
}

#[test]
fn extract_matches_a_bracketed_name_byte_exactly() {
    let dir = tempdir().unwrap();
    let archive = extract_fixture(dir.path());
    let output = cadmpeg()
        .args([
            "inspect",
            "extract",
            archive.to_str().unwrap(),
            "Body[Active].brp",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"bracketed payload");
}

#[test]
fn extract_names_candidates_when_the_member_is_missing() {
    let dir = tempdir().unwrap();
    let archive = extract_fixture(dir.path());
    cadmpeg()
        .args([
            "inspect",
            "extract",
            archive.to_str().unwrap(),
            "streams.dat",
        ])
        .assert()
        .code(2)
        .stderr(
            predicate::str::contains("'Design/Streams.dat'")
                .and(predicate::str::contains("cadmpeg inspect container")),
        );
}

#[test]
fn extract_refuses_to_overwrite_without_force() {
    let dir = tempdir().unwrap();
    let archive = extract_fixture(dir.path());
    let out = dir.path().join("existing.bin");
    fs::write(&out, b"precious").unwrap();

    cadmpeg()
        .args([
            "inspect",
            "extract",
            archive.to_str().unwrap(),
            "Body[Active].brp",
            "-o",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--force"));
    assert_eq!(fs::read(&out).unwrap(), b"precious");

    cadmpeg()
        .args([
            "inspect",
            "extract",
            archive.to_str().unwrap(),
            "Body[Active].brp",
            "-o",
            out.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();
    assert_eq!(fs::read(&out).unwrap(), b"bracketed payload");
}

#[test]
fn extract_is_reachable_through_the_bytes_group() {
    let dir = tempdir().unwrap();
    let archive = extract_fixture(dir.path());
    let output = cadmpeg()
        .args([
            "inspect",
            "bytes",
            "extract",
            archive.to_str().unwrap(),
            "Body[Active].brp",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"bracketed payload");
}

#[test]
fn input_flag_reaches_the_same_file_as_the_positional() {
    let dir = tempdir().unwrap();
    let bytes: Vec<u8> = (0u8..64).collect();
    let counter = write(dir.path(), "counter.bin", &bytes);
    let path = counter.to_str().unwrap();

    // Byte-identical stdout under either spelling, per single-input tool.
    for args in [
        vec!["inspect", "hex", "--len", "0x10"],
        vec!["inspect", "read", "--type", "u8", "-n", "2"],
        vec!["inspect", "strings", "--min", "1"],
        vec!["inspect", "struct", "--layout", "u8:a"],
        vec!["inspect", "find", "--hex", "05"],
    ] {
        let positional = cadmpeg().args(&args).arg(path).output().unwrap();
        let mut flagged = args.clone();
        flagged.push("--input");
        flagged.push(path);
        let via_flag = cadmpeg().args(&flagged).output().unwrap();
        assert!(positional.status.success(), "{args:?}");
        assert!(via_flag.status.success(), "{args:?} --input");
        assert_eq!(positional.stdout, via_flag.stdout, "{args:?}");
    }
}

#[test]
fn input_flag_and_positional_together_conflict() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "some.bin", b"x");
    let path = file.to_str().unwrap();
    cadmpeg()
        .args(["inspect", "hex", path, "--input", path])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn find_with_input_flag_still_teaches_a_misplaced_pattern() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "some.bin", b"document here");
    let path = file.to_str().unwrap();
    for extra in [vec!["document"], vec![]] {
        let mut args = vec!["inspect", "find", "--input", path];
        args.extend(&extra);
        let assert = cadmpeg().args(&args).assert();
        if extra.is_empty() {
            assert
                .code(2)
                .stderr(predicate::str::contains("--hex, --ascii, or --utf16le"));
        } else {
            assert.code(2).stderr(predicate::str::contains(
                "`document` is an extra positional argument",
            ));
        }
    }
}

#[test]
fn inspect_input_flag_positional_and_subcommand_interplay() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "plain.bin", b"not a container");
    let path = file.to_str().unwrap();

    cadmpeg()
        .args(["inspect", "--input", path])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no codec recognized"));

    cadmpeg()
        .args(["inspect", "hex", path, "--len", "1"])
        .assert()
        .success();

    cadmpeg()
        .args(["inspect", "--input", path, "hex"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));

    // Both spellings at once conflict.
    cadmpeg()
        .args(["inspect", path, "--input", path])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));

    // Bare `inspect` still demands an input.
    cadmpeg()
        .args(["inspect"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("required arguments"));
}

#[test]
fn container_json_lists_entries_under_the_envelope() {
    let dir = tempdir().unwrap();
    let archive = extract_fixture(dir.path());
    let output = cadmpeg()
        .args(["inspect", "container", "--json", archive.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 8);
    assert_eq!(value["command"], "inspect container");
    assert!(value["generator"].as_str().unwrap().starts_with("cadmpeg "));
    assert_eq!(value["container_kind"], "zip");
    let entries = value["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    // Raw names in JSON: no shell quoting.
    assert_eq!(entries[0]["name"], "Body[Active].brp");
    assert_eq!(entries[0]["compression"], "stored");
    assert_eq!(entries[0]["uncompressed_size"], 17);
}

#[test]
fn container_lists_cfb_directory_rows() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "doc.cfb", &compound_fixture());

    cadmpeg()
        .args(["inspect", "container", file.to_str().unwrap()])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("stream")
                .and(predicate::str::contains("Payload"))
                .and(predicate::str::contains("4096")),
        );

    let output = cadmpeg()
        .args(["inspect", "container", "--json", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 8);
    assert_eq!(value["command"], "inspect container");
    assert_eq!(value["container_kind"], "cfb");
}

#[test]
fn step_is_an_alias_of_stride() {
    let dir = tempdir().unwrap();
    let bytes: Vec<u8> = (0u8..16).collect();
    let file = write(dir.path(), "counter.bin", &bytes);
    let path = file.to_str().unwrap();
    let stride = cadmpeg()
        .args([
            "inspect", "read", path, "--type", "u8", "-n", "3", "--stride", "4",
        ])
        .output()
        .unwrap();
    let step = cadmpeg()
        .args([
            "inspect", "read", path, "--type", "u8", "-n", "3", "--step", "4",
        ])
        .output()
        .unwrap();
    assert!(stride.status.success());
    assert_eq!(stride.stdout, step.stdout);
}

#[test]
fn read_type_text_and_hex_guesses_teach_the_right_tool() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "some.bin", b"x");
    let path = file.to_str().unwrap();

    for (guess, expected) in [
        ("ascii", "cadmpeg inspect strings"),
        ("text", "cadmpeg inspect strings"),
        ("hex", "cadmpeg inspect hex"),
    ] {
        cadmpeg()
            .args(["inspect", "read", path, "--type", guess])
            .assert()
            .code(2)
            .stderr(predicate::str::contains(expected));
    }

    // The scalar list still renders in help.
    cadmpeg()
        .args(["inspect", "read", "-h"])
        .assert()
        .success()
        .stdout(predicate::str::contains("possible values: u8, i8"));
}

#[test]
fn find_type_guess_names_the_encoding_flags() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "some.bin", b"x");
    let path = file.to_str().unwrap();
    for (guess, flag) in [
        ("ascii", "--ascii TEXT"),
        ("hex", "--hex PATTERN"),
        ("utf16", "--utf16le TEXT"),
    ] {
        cadmpeg()
            .args(["inspect", "find", path, "--type", guess])
            .assert()
            .code(2)
            .stderr(predicate::str::contains(flag));
    }
}

#[test]
fn find_context_prints_a_window_around_each_hit() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "ctx.bin", b"AAAAneedleBBBB");
    let path = file.to_str().unwrap();

    cadmpeg()
        .args(["inspect", "find", path, "--ascii", "needle"])
        .assert()
        .success()
        .stdout(
            "pattern: ascii \"needle\" (6 bytes)  hits: 1\n\
             0x00000004  4\n",
        );

    // --context 4 appends a dump spanning 4 bytes before and after the hit.
    cadmpeg()
        .args([
            "inspect",
            "find",
            path,
            "--ascii",
            "needle",
            "--context",
            "4",
        ])
        .assert()
        .success()
        .stdout(
            "pattern: ascii \"needle\" (6 bytes)  hits: 1\n\
             0x00000004  4\n\
             00000000  41 41 41 41 6e 65 65 64  6c 65 42 42 42 42        \
             |AAAAneedleBBBB|\n",
        );
}

#[test]
fn find_json_emits_the_versioned_envelope() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "probe.bin", b"AAAAneedleBBBBneedle");
    let output = cadmpeg()
        .args([
            "inspect",
            "find",
            file.to_str().unwrap(),
            "--ascii",
            "needle",
            "--max",
            "0",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "inspect find");
    assert!(value["generator"].as_str().unwrap().starts_with("cadmpeg "));
    assert_eq!(value["hits"], serde_json::json!([4, 14]));
    assert_eq!(value["truncated"], false);
}

#[test]
fn json_on_a_tool_without_a_json_form_teaches_where_json_lives() {
    let dir = tempdir().unwrap();
    let file = write(dir.path(), "probe.bin", b"AAAA");
    for tool in ["hex", "strings"] {
        let output = cadmpeg()
            .args(["inspect", tool, file.to_str().unwrap(), "--json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{tool}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("inspect find --json"), "{tool}: {stderr}");
    }
}
