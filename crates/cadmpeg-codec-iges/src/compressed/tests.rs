// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::{wire, EditableDecodeResult};

use super::{normalize, parse_data_entity, parse_directory_record, parse_field_specs, DataEntity};
use crate::loss::IgesLossCode;
use crate::test_support::test_curves_and_surfaces::{point_file, point_file_with_global};
use crate::test_support::{global_with_version_flag, only_match};
use crate::IgesCodec;
use crate::IgesVersion;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::dialect::Admission;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::export::{FidelityResolution, WritePath};
use std::fmt::Write as _;
use std::io::Cursor;

fn normalize_for_test(source: &[u8]) -> Result<Vec<u8>, cadmpeg_core::CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(source, &arena, &policy)?;
    normalize(source, &ctx)
}

fn normalize_with_policy(source: &[u8], policy: &DecodePolicy) -> Result<Vec<u8>, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, policy)?;
    normalize(source, &ctx)
}

#[test]
fn compressed_sparse_directory_shares_unchanged_field_values() {
    let source = compressed_points_file();
    let owned_lines = source_lines(&source);
    let lines = owned_lines.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let first_directory = lines
        .iter()
        .position(|line| line.first() == Some(&b'D'))
        .unwrap();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&source, &arena, &policy).unwrap();
    let (first, next) = parse_data_entity(&lines, first_directory, None, b',', b';', &ctx).unwrap();
    let (second, _) =
        parse_data_entity(&lines, next, Some(&first.fields), b',', b';', &ctx).unwrap();

    assert!(std::rc::Rc::ptr_eq(&first.fields.0[0], &second.fields.0[0]));
    assert!(!std::rc::Rc::ptr_eq(
        &first.fields.0[14],
        &second.fields.0[14]
    ));
    assert_eq!(
        first.fields.get(super::CompressedField::Shared(
            crate::directory::DirectoryFieldSlot::Label
        )),
        b"POINT"
    );
    assert_eq!(
        second.fields.get(super::CompressedField::Shared(
            crate::directory::DirectoryFieldSlot::Label
        )),
        b"SECOND"
    );
}

#[test]
fn compressed_directory_specs_refuse_materialized_limit_before_copy() {
    let lines = [b"D1@1_116;".as_slice()];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 5;
    let (ctx, _) = DecodeContext::from_root_bytes(lines[0], &arena, &policy).unwrap();
    let error = parse_directory_record(&lines, 0, b';', &ctx).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "iges_compressed_directory_spec_bytes")
    );
}

#[test]
fn compressed_line_index_refuses_collection_limit_before_growth() {
    let source = compressed_points_file();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = u64::try_from(source_lines(&source).len() - 1).unwrap();
    let error = normalize_with_policy(&source, &policy).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "iges_compressed_ascii_lines")
    );
}

#[test]
fn compressed_global_workspace_refuses_materialized_limit_before_copy() {
    let source = compressed_points_file();
    let global_cards = source_lines(&source)
        .iter()
        .filter(|line| line.get(72) == Some(&b'G'))
        .count();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::try_from(global_cards * 2 * 72 - 1).unwrap();
    let error = normalize_with_policy(&source, &policy).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "iges_compressed_global_stream")
    );
}

#[test]
fn compressed_directory_field_refuses_retained_limit_before_copy() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(b"@1_116", &arena, &policy).unwrap();
    let error = parse_field_specs(b"@1_116", &ctx).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "iges_compressed_directory_field")
    );
}

#[test]
fn compressed_entity_refuses_retained_limit_before_record_allocation() {
    let source = compressed_points_file();
    let mut policy = DecodePolicy::service();
    let line_headers = source_lines(&source).len() * std::mem::size_of::<&[u8]>();
    policy.limits.max_retained_bytes =
        u64::try_from(line_headers + std::mem::size_of::<DataEntity>() - 1).unwrap();
    let error = normalize_with_policy(&source, &policy).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "iges_compressed_entity_record")
    );
}

#[test]
fn compressed_entity_index_refuses_collection_limit_before_growth() {
    let source = compressed_points_file();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = u64::try_from(source_lines(&source).len()).unwrap();
    let error = normalize_with_policy(&source, &policy).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "iges_compressed_entities")
    );
}

#[test]
fn compressed_normalization_refuses_work_before_directory_loop() {
    let source = compressed_points_file();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::try_from(source.len() - 1).unwrap();
    let error = normalize_with_policy(&source, &policy).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "iges_compressed_ascii_normalization")
    );
}

#[test]
fn compressed_parameter_lines_refuse_collection_limit_before_allocation() {
    let lines = [
        b"D1@1_116@3_0@4_0@5_0@6_0@7_0@8_0@9_00000000".as_slice(),
        b"@12_0@13_0@14_1@15_0@16_@17_@18_POINT@19_0;".as_slice(),
        b"116,1.0,2.0,3.0;".as_slice(),
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = parse_data_entity(&lines, 0, None, b',', b';', &ctx).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "iges_compressed_parameter_lines")
    );
}

#[test]
fn compressed_parameter_starts_refuse_collection_limit_before_allocation() {
    let source = compressed_points_file();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = u64::try_from(source_lines(&source).len() + 4).unwrap();
    let error = normalize_with_policy(&source, &policy).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.operation == "iges_compressed_parameter_starts")
    );
}

#[test]
fn compressed_normalized_output_refuses_retained_limit_before_allocation() {
    let source = compressed_points_file();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(source.len() * 3).unwrap();
    let error = normalize_with_policy(&source, &policy).unwrap_err();
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "iges_compressed_normalized_output"),
        "{error:#?}"
    );
}

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
    let normalized = normalize_for_test(&source).unwrap();
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

    let result = EditableDecodeResult::from(
        IgesCodec
            .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
            .unwrap(),
    );
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
    assert!(matches!(
        plan.report().write_path(),
        WritePath::Synthesized { .. }
    ));
    assert_eq!(
        &wire::field::<cadmpeg_ir::report::export::FidelityResolution>(
            plan.report().write_path(),
            "fidelity"
        ),
        &FidelityResolution::NotConsumed {}
    );
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
    let result = EditableDecodeResult::from(
        IgesCodec
            .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
            .unwrap(),
    );
    let plan = IgesCodec
        .plan(
            EncodeInput::new(result.ir(), Some(result.source_fidelity())),
            TargetRequest::Inherit,
        )
        .unwrap();

    assert!(matches!(
        plan.report().write_path(),
        WritePath::VerbatimReplay { .. }
    ));
    assert_eq!(
        wire::field_or_default::<Option<cadmpeg_core::dialect::DialectId>>(
            plan.report(),
            "identity/target"
        )
        .as_ref()
        .map(ToString::to_string),
        Some("iges:5.3-compressed-ascii".to_owned())
    );
    assert!(matches!(
        &wire::field::<cadmpeg_ir::report::export::FidelityResolution>(
            plan.report().write_path(),
            "fidelity"
        ),
        FidelityResolution::Replayed {}
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

    let normalized = normalize_for_test(&source).unwrap();
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (entity, next) = parse_data_entity(&lines, 0, None, b',', b';', &ctx).unwrap();
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = parse_directory_record(&lines, 0, b';', &ctx).unwrap_err();
    assert!(error.to_string().contains("Directory field 2 is redundant"));
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
    assert_eq!(matched.admission(), &Admission::Admitted);
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
    let source = compressed_points_file_with_global(&global_with_version_flag("4"));
    let decoded = IgesCodec
        .decode(&mut Cursor::new(source.clone()), &DecodeOptions::default())
        .unwrap();

    let matched = only_match(decoded.report().dialects());
    assert_eq!(matched.dialect().as_str(), "iges:unknown");
    assert_eq!(
        matched.using(),
        Some(cadmpeg_core::dialect_id!("iges:5.3-compressed-ascii"))
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

#[test]
fn compressed_reserved_fields_use_fixed_directory_right_justification() {
    let source = std::str::from_utf8(include_bytes!(
        "../../tests/golden/fixtures/compressed_ascii_5_3.igs"
    ))
    .unwrap()
    .replace("@16_@17_", "@16_LEFT@17_RIGHT");
    let normalized = normalize_for_test(source.as_bytes()).unwrap();
    let result = IgesCodec
        .decode(&mut Cursor::new(normalized), &DecodeOptions::default())
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(
        native.arenas()["entities"][0].fields()["reserved"],
        serde_json::json!([b"    LEFT", b"   RIGHT"])
    );
}
