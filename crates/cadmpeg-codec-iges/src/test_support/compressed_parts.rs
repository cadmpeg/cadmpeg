// SPDX-License-Identifier: Apache-2.0
//! Sparse source records with explicit and inherited mandatory Directory values.

use super::test_curves_and_surfaces::point_file;

pub(crate) fn points(broken: bool, inherited_failure: bool) -> Vec<u8> {
    let fixed = point_file();
    let lines: Vec<_> = fixed
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    let directory = lines
        .iter()
        .position(|line| line.get(72) == Some(&b'D'))
        .unwrap();
    let terminate = lines
        .iter()
        .position(|line| line.get(72) == Some(&b'T'))
        .unwrap();
    let mut out = vec![b' '; 80];
    out[72] = b'C';
    out[79] = b'1';
    out.extend_from_slice(b"\r\n");
    for line in &lines[..directory] {
        out.extend_from_slice(line);
        out.extend_from_slice(b"\r\n");
    }
    let mut specs = Vec::new();
    for number in [1usize, 3, 4, 5, 6, 7, 8, 9, 12, 13, 14, 15, 16, 17, 18, 19] {
        let (line, slot) = if number <= 9 {
            (lines[directory], number - 1)
        } else {
            (lines[directory + 1], number - 11)
        };
        let mut value = std::str::from_utf8(&line[slot * 8..slot * 8 + 8])
            .unwrap()
            .trim()
            .to_owned();
        if number == 3 {
            value = if broken { "x" } else { "0" }.to_owned();
        }
        specs.push(format!("@{number}_{value}"));
    }
    out.extend_from_slice(
        format!(
            "D1{}\r\n{};\r\n116,1.0,2.0,3.0;\r\n",
            specs[..8].concat(),
            specs[8..].concat()
        )
        .as_bytes(),
    );
    if inherited_failure {
        out.extend_from_slice(
            b"D3@18_INHERIT;\r\n116,4.0,5.0,6.0;\r\nD5@3_0@18_PEER;\r\n116,7.0,8.0,9.0;\r\n",
        );
    } else {
        out.extend_from_slice(b"D3@3_0@18_PEER;\r\n116,4.0,5.0,6.0;\r\n");
    }
    out.extend_from_slice(lines[terminate]);
    out.extend_from_slice(b"\r\n");
    out
}

pub(crate) fn without_initial_field(field: &[u8], inherited: bool) -> (Vec<u8>, Vec<u8>) {
    let original = points(false, inherited);
    let at = original
        .windows(field.len())
        .position(|bytes| bytes == field)
        .expect("authored first Directory field");
    let mut missing = original.clone();
    drop(missing.drain(at..at + field.len()));
    (original, missing)
}

pub(crate) fn with_unreadable_initial_count() -> (Vec<u8>, Vec<u8>) {
    let original = points(false, false);
    let field = b"@14_1";
    let at = original
        .windows(field.len())
        .position(|bytes| bytes == field)
        .expect("authored Parameter line count");
    let mut unreadable = original.clone();
    unreadable[at + field.len() - 1] = b'x';
    (original, unreadable)
}
