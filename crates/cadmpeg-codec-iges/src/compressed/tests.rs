// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use crate::loss::IgesLossCode;
use crate::test_support::{point_file, point_file_with_global};
use crate::IgesCodec;
use crate::IgesVersion;
use cadmpeg_core::dialect::{Admission, DialectId, DialectLayers, DialectMatch};
use cadmpeg_ir::codec::TargetRequest;
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

    // Compressed ASCII is not the dialect this writer synthesizes, so an
    // explicit Fixed ASCII target declines replay and charges displacement.
    // The gate used to compare the version alone and replayed the
    // compressed bytes while the plan claimed Fixed ASCII.
    let plan = IgesCodec
        .plan(
            EncodeInput::new(result.ir(), Some(result.source_fidelity())),
            TargetRequest::Explicit(IgesVersion::V5_3.descriptor().id.as_str()),
        )
        .unwrap();
    assert_eq!(plan.report().write_path, WritePath::Synthesized);
    assert_eq!(&plan.report().fidelity, &FidelityResolution::NotConsumed);
    let displacement = plan
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == IgesLossCode::SourceDialectDisplaced.kind())
        .expect("representation displacement is charged");
    assert!(displacement.message.contains("iges:5.3-compressed-ascii"));
    assert!(displacement.message.contains("iges:5.3-fixed-ascii"));
}

/// Preservation is not synthesis: a Compressed ASCII source replays its own
/// bytes under an inherit request even though no input makes the semantic
/// writer emit Compressed ASCII.
///
/// The retained image is the original bytes, so the resolved dialect is the
/// source's by construction and the replay law admits the copy. This is the
/// capability an explicit Fixed ASCII target cannot ask for.
#[test]
fn compressed_ascii_replays_its_own_bytes_under_an_inherit_request() {
    let source = compressed_points_file();
    let result = IgesCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .unwrap();
    let plan = IgesCodec
        .plan(
            EncodeInput::new(result.ir(), Some(result.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();

    assert_eq!(plan.report().write_path, WritePath::VerbatimReplay);
    assert_eq!(
        plan.report().target().map(ToString::to_string),
        Some("iges:5.3-compressed-ascii".to_owned())
    );
    assert!(matches!(
        &plan.report().fidelity,
        FidelityResolution::Replayed
    ));
    let mut written = Vec::new();
    plan.write_to(&mut written).unwrap();
    assert_eq!(written, source);
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
            result.report().dialects().unwrap().primary().declared()["effective_version"],
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

/// A 26-field Global record carrying `version_flag` in field 23.
fn compressed_global_with_version_flag(version_flag: &str) -> Vec<u8> {
    format!(
        "1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,\
         2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,{version_flag},0,0H,0H;"
    )
    .into_bytes()
}

/// The one dialect match of a report or summary.
fn only_match(dialects: Option<&DialectLayers>) -> &DialectMatch {
    let layers = dialects.expect("IGES reports dialect layers");
    assert_eq!(layers.iter().count(), 1, "{dialects:#?}");
    assert_eq!(layers.primary().format(), "iges");
    layers.primary()
}

#[test]
fn compressed_ascii_classifies_into_its_own_representation_row() {
    // The registry states Compressed ASCII at IGES 5.3, so a compressed file at
    // flag 11 names that row rather than the Fixed ASCII one it normalizes to.
    let source = compressed_points_file();
    let decoded = IgesCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .unwrap();

    let matched = only_match(decoded.report().dialects());
    assert_eq!(matched.dialect().as_str(), "iges:5.3-compressed-ascii");
    assert_eq!(matched.admission(), Admission::Admitted);
    assert_eq!(matched.declared()["representation"], "compressed-ascii");

    let source_meta = decoded.ir().source.as_ref().unwrap();
    assert_eq!(source_meta.dialect(), Some(matched));

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(source),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert_eq!(only_match(summary.dialects()), matched);
}

#[test]
fn compressed_ascii_at_a_version_with_no_row_classifies_into_the_totality_row() {
    // The registry declines to invent Compressed ASCII rows below IGES 4.0: the
    // IGES 3.0 specification would witness them. A compressed file at flag 4
    // therefore satisfies no row, which is the totality row's whole purpose.
    let source = compressed_points_file_with_global(&compressed_global_with_version_flag("4"));
    let decoded = IgesCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .unwrap();

    let matched = only_match(decoded.report().dialects());
    assert_eq!(matched.dialect().as_str(), "iges:unknown");
    assert_eq!(
        matched.admission(),
        Admission::AdmittedUnverified {
            using: Some(DialectId::pinned("iges:5.3-compressed-ascii")),
        }
    );
    assert_eq!(matched.declared()["representation"], "compressed-ascii");
    assert_eq!(matched.declared()["version_flag"], "4");
    assert_eq!(matched.declared()["effective_version"], "3.0");
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::IgesLossCode::SourceDialectUnverified.kind()));

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(source),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();
    assert_eq!(only_match(summary.dialects()), matched);
    assert!(summary.notes.contains(&"iges_version=unverified".into()));
    assert!(summary
        .notes
        .contains(&"iges_declared_version_flag=4".into()));
    assert!(summary.notes.contains(&"iges_effective_version=3.0".into()));
}
