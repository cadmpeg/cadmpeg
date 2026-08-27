// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use crate::test_support::{point_file, point_file_with_global};
use crate::{IgesCodec, IgesEncoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::report::{FidelityResolution, WritePath};
use std::fmt::Write as _;
use std::io::Cursor;

fn source_lines(bytes: &[u8]) -> Vec<Vec<u8>> {
    bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line).to_vec())
        .collect()
}

fn de_value(lines: &[Vec<u8>], field: usize) -> String {
    let directory_start = lines
        .iter()
        .position(|line| line.get(72) == Some(&b'D'))
        .unwrap();
    let line = if field <= 9 {
        &lines[directory_start]
    } else {
        &lines[directory_start + 1]
    };
    let index = if field <= 9 { field - 1 } else { field - 11 };
    String::from_utf8(line[index * 8..(index + 1) * 8].to_vec())
        .unwrap()
        .trim()
        .to_owned()
}

fn field_specs(lines: &[Vec<u8>], fields: &[usize]) -> String {
    let mut specs = String::new();
    for field in fields {
        write!(specs, "@{field}_{}", de_value(lines, *field)).unwrap();
    }
    specs
}

fn compressed_points_file_with_syntax(
    global: &[u8],
    parameter_delimiter: u8,
    record_delimiter: u8,
) -> Vec<u8> {
    let parameter_delimiter = char::from(parameter_delimiter);
    let record_delimiter = char::from(record_delimiter);
    let fixed = source_lines(&point_file_with_global(global));
    let directory_start = fixed
        .iter()
        .position(|line| line.get(72) == Some(&b'D'))
        .unwrap();
    let terminate = fixed
        .iter()
        .position(|line| line.get(72) == Some(&b'T'))
        .unwrap();
    let mut source = Vec::new();
    let mut flag = [b' '; 80];
    flag[72] = b'C';
    flag[79] = b'1';
    source.extend_from_slice(&flag);
    source.extend_from_slice(b"\r\n");
    for line in &fixed[..directory_start] {
        source.extend_from_slice(line);
        source.extend_from_slice(b"\r\n");
    }

    let fields = [1, 3, 4, 5, 6, 7, 8, 9, 12, 13, 14, 15, 16, 17, 18, 19];
    let first_specs = fields
        .into_iter()
        .map(|field| format!("@{field}_{}", de_value(&fixed, field)))
        .collect::<Vec<_>>();
    source.extend_from_slice(format!("D1{}\r\n", first_specs[..8].concat()).as_bytes());
    source.extend_from_slice(
        format!("{}{}\r\n", first_specs[8..].concat(), record_delimiter).as_bytes(),
    );
    source.extend_from_slice(
        format!(
            "116{parameter_delimiter}1.0{parameter_delimiter}2.0{parameter_delimiter}3.0{record_delimiter}\r\n"
        )
        .as_bytes(),
    );
    source.extend_from_slice(format!("D3@18_SECOND{record_delimiter}\r\n").as_bytes());
    source.extend_from_slice(
        format!(
            "116{parameter_delimiter}4.0{parameter_delimiter}5.0{parameter_delimiter}6.0{record_delimiter}\r\n"
        )
        .as_bytes(),
    );
    source.extend_from_slice(&fixed[terminate]);
    source.extend_from_slice(b"\r\n");
    source
}
fn compressed_points_file() -> Vec<u8> {
    compressed_points_file_with_syntax(
        b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,11,0,0H,0H;",
        b',',
        b';',
    )
}

fn compressed_points_file_with_global(global: &[u8]) -> Vec<u8> {
    compressed_points_file_with_syntax(global, b',', b';')
}

#[test]
fn compressed_ascii_derives_fixed_cards_and_inherits_directory_fields() {
    let source = compressed_points_file();
    let normalized = normalize(&source, None).unwrap();
    let lines = source_lines(&normalized);
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.get(72) == Some(&b'D'))
            .count(),
        4
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.get(72) == Some(&b'P'))
            .count(),
        2
    );
    let directory_start = lines
        .iter()
        .position(|line| line.get(72) == Some(&b'D'))
        .unwrap();
    assert_eq!(lines[directory_start][..8], *b"     116");
    assert_eq!(lines[directory_start + 3][56..64], *b"  SECOND");
    assert_eq!(lines[directory_start + 4][64..72], *b"       1");
    assert_eq!(lines[directory_start + 5][64..72], *b"       3");

    let result = IgesCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 2);
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["representation"],
        "compressed-ascii"
    );

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(source.clone()),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert_eq!(summary.container_kind, "compressed-ascii");
    assert!(summary
        .notes
        .contains(&"normalized_representation=compressed-ascii".into()));

    // Compressed ASCII is not the dialect this writer synthesizes, so the
    // replay gate declines and the report names both dialects. The gate used to
    // compare the version alone and replayed the compressed bytes while the plan
    // claimed Fixed ASCII. `TargetRequest::Inherit` (design section 8.1) is what
    // asks for preservation; the encoder trait does not carry it yet.
    let plan = IgesEncoder::default()
        .plan(EncodeInput {
            ir: result.ir(),
            fidelity: Some(result.source_fidelity()),
        })
        .unwrap();
    assert_eq!(plan.write_path(), WritePath::Synthesized);
    match plan.fidelity_resolution() {
        FidelityResolution::Degraded { reason } => {
            assert!(
                reason.contains("source is iges:5.3-compressed-ascii"),
                "{reason}"
            );
            assert!(
                reason.contains("target is iges:5.3-fixed-ascii"),
                "{reason}"
            );
        }
        other => panic!("a representation mismatch must degrade: {other:?}"),
    }
}

#[test]
fn compressed_ascii_accepts_directory_specifiers_on_multiple_lines() {
    let fixed = source_lines(&point_file());
    let directory_start = fixed
        .iter()
        .position(|line| line.get(72) == Some(&b'D'))
        .unwrap();
    let terminate = fixed
        .iter()
        .position(|line| line.get(72) == Some(&b'T'))
        .unwrap();
    let fields = [1, 3, 4, 5, 6, 7, 8, 9];
    let first = field_specs(&fixed, &fields);
    let second_fields = [12, 13, 14, 15, 16, 17, 18, 19];
    let second = field_specs(&fixed, &second_fields);

    let mut source = Vec::new();
    let mut flag = [b' '; 80];
    flag[72] = b'C';
    source.extend_from_slice(&flag);
    source.push(b'\n');
    for line in &fixed[..directory_start] {
        source.extend_from_slice(line);
        source.push(b'\n');
    }
    source.extend_from_slice(format!("D1{first}\n").as_bytes());
    source.extend_from_slice(format!("{second};\n").as_bytes());
    source.extend_from_slice(b"116,1.0,2.0,3.0;\n");
    source.extend_from_slice(&fixed[terminate]);
    source.push(b'\n');

    let normalized = normalize(&source, None).unwrap();
    assert_eq!(
        source_lines(&normalized)
            .iter()
            .filter(|line| line.get(72) == Some(&b'P'))
            .count(),
        1
    );
}

#[test]
fn compressed_ascii_preserves_v4_and_v5_0_profiles() {
    let globals = [
        (
            "4.0",
            b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,6,0;".as_slice(),
        ),
        (
            "5.0",
            b"1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,,1.0,2,2HMM,1,1.0,13H260714.000000,0.001,1000.0,6Hauthor,3Horg,8,0,0H;".as_slice(),
        ),
    ];
    for (version, global) in globals {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(compressed_points_file_with_global(global)),
                &DecodeOptions::default(),
            )
            .unwrap();
        assert_eq!(result.ir().model.points.len(), 2, "{version}");
        assert_eq!(
            result.ir().source.as_ref().unwrap().attributes["iges_version"],
            version
        );
    }
}

#[test]
fn compressed_ascii_record_termination_ignores_hollerith_payload_delimiters() {
    let lines = [
        b"D1@1_116@3_0@4_0@5_0@6_0@7_0@8_0@9_00000000".as_slice(),
        b"@12_0@13_0@14_1@15_0@16_@17_@18_POINT@19_0;".as_slice(),
        b"116,1.0,2.0,3H;X;;".as_slice(),
    ];
    let previous = std::array::from_fn(|_| None);
    let (entity, next) = parse_data_entity(&lines, 0, &previous, true, b',', b';').unwrap();
    assert_eq!(next, 3);
    assert_eq!(entity.parameter_lines.len(), 1);
}

#[test]
fn compressed_ascii_accepts_non_default_delimiters() {
    let source = compressed_points_file_with_syntax(
        b"1H||1H!|7Hproduct|8Hpart.igs|7Hcadmpeg|3H0.1|32|38|6|308|15|0H|1.0|2|2HMM|1|1.0|15H20260714.000000|0.001|1000.0|6Hauthor|3Horg|11|0|0H|0H!",
        b'|',
        b'!',
    );
    let result = IgesCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.points.len(), 2);
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["parameter_delimiter"],
        "|"
    );
    assert_eq!(
        result.ir().source.as_ref().unwrap().attributes["record_delimiter"],
        "!"
    );
}

#[test]
fn compressed_ascii_rejects_redundant_directory_specifiers() {
    let lines = [b"D1@1_116@2_1;".as_slice()];
    let error = parse_directory_record(&lines, 0, b';').unwrap_err();
    assert!(error.to_string().contains("Directory field 2 is redundant"));
}
