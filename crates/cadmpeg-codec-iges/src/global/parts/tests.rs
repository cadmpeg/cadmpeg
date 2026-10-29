// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::PartOutput;

use super::parse;
use crate::card::{self, Section};
use crate::test_support::test_cards::fixed_ascii_with_global;

fn fields() -> Vec<String> {
    [
        "1H,",
        "1H;",
        "7Hproduct",
        "8Hpart.igs",
        "7Hcadmpeg",
        "3H0.1",
        "32",
        "38",
        "6",
        "308",
        "15",
        "0H",
        "1.0",
        "2",
        "2HMM",
        "1",
        "1.0",
        "15H20260714.000000",
        "0.001",
        "1000.0",
        "6Hauthor",
        "3Horg",
        "11",
        "0",
        "0H",
        "0H",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn file(fields: &[String]) -> Vec<u8> {
    fixed_ascii_with_global(format!("{};", fields.join(",")).as_bytes())
}

#[test]
fn actual_global_callback_preserves_values_losses_and_independent_framing() {
    let bytes = file(&fields());
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let scan = card::scan_source(root).unwrap();
    let (expected, expected_losses) = crate::global::parse(&scan).unwrap();
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let (actual, losses) = parse(&scan, &ctx, &mut output).unwrap();
    assert_eq!(actual.sender_product(), Some("product"));
    assert_eq!(actual.native_file_name(), Some("part.igs"));
    assert_eq!(actual.units_name(), Some("MM"));
    assert_eq!(actual.declared_version_flag(), 11);
    assert_eq!(actual.summary_notes(), expected.summary_notes());
    assert_eq!(
        serde_json::to_value(losses).unwrap(),
        serde_json::to_value(expected_losses).unwrap()
    );
    let (_, _, fidelity, losses) = output.into_parts();
    assert!(losses.is_empty());
    assert!(fidelity.retained_records().is_empty());
}

#[test]
fn cross_card_metadata_reads_use_actual_composite_value_bytes_only() {
    let payload = [b'a'; 80];
    let mut logical = b",,80H".to_vec();
    logical.extend_from_slice(&payload);
    logical.extend_from_slice(b",7;");
    let original = fixed_ascii_with_global(&logical);
    let mut bytes = b"pre".to_vec();
    bytes.extend_from_slice(&original);
    bytes.extend_from_slice(b"post");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(3, bytes.len() - 4).unwrap();
    let scan = card::scan_source(source).unwrap();
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let (global, _) = parse(&scan, &ctx, &mut output).unwrap();
    assert_eq!(
        global.sender_product(),
        Some(std::str::from_utf8(&payload).unwrap())
    );
    assert_eq!(global.native_file_name(), None);
    let (_, _, fidelity, losses) = output.into_parts();
    assert!(fidelity.retained_records().is_empty());
    assert!(losses.is_empty());
}

#[test]
fn discarded_descriptive_and_unadmitted_metadata_get_no_projection_credit() {
    let mut fields = fields();
    fields[2] = "5Hbad\u{7f}x".into();
    let bytes = file(&fields);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let scan = card::scan_source(root).unwrap();
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let (global, _) = parse(&scan, &ctx, &mut output).unwrap();
    assert_eq!(global.sender_product(), None);
    assert_eq!(global.native_file_name(), Some("part.igs"));
}

#[test]
fn mandatory_global_failure_has_one_shared_actual_source_home_and_no_reads() {
    let bytes = fixed_ascii_with_global(b"X;");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let scan = card::scan_source(root).unwrap();
    let original_error = crate::global::parse(&scan).unwrap_err();
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    // #222 C.5: the delimiter declarations frame the section, so the section
    // is given up. Every Global value is absent and the delimiters are the
    // IGES 5.3 §2.2.4.3.1-2 defaults.
    let (global, _) = parse(&scan, &ctx, &mut output).unwrap();
    assert_eq!(global.parameter_delimiter, b',');
    assert_eq!(global.record_delimiter, b';');
    assert_eq!(global.sender_product(), None);
    assert_eq!(global.native_file_name(), None);
    let (_, native, fidelity, losses) = output.into_parts();
    assert!(native.namespace("iges").is_none());
    let global = scan.section(Section::Global).next().unwrap().1;
    let start = usize::try_from(global.offset).unwrap();
    assert_eq!(fidelity.retained_records().len(), 1);
    let (_, section) = fidelity.retained_records().first_key_value().unwrap();
    assert!(!section.untyped().is_empty());
    assert_eq!(section.data(), Some(&bytes[start..start + 81]));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(losses[0].message.contains(&original_error.to_string()));
}

#[test]
fn unreadable_descriptive_suffix_keeps_original_preceding_values_and_losses() {
    let mut fields = fields();
    fields[23] = "99999Hx".into();
    let bytes = file(&fields);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let scan = card::scan_source(root).unwrap();
    let (expected, expected_losses) = crate::global::parse(&scan).unwrap();
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let (actual, losses) = parse(&scan, &ctx, &mut output).unwrap();
    assert_eq!(actual.sender_product(), Some("product"));
    assert_eq!(actual.declared_version_flag(), 11);
    assert_eq!(actual.summary_notes(), expected.summary_notes());
    // #222 C.5: the field whose end is unknown is a given-up part. Its part
    // loss replaces the unreadable-suffix framing loss of the partless parse.
    let suffix = expected_losses
        .iter()
        .position(|loss| {
            loss.code == crate::loss::IgesLossCode::GlobalNoncanonicalFraming.kind()
                && loss.message.contains("unreadable suffix")
        })
        .unwrap();
    let mut expected_losses = expected_losses;
    expected_losses.remove(suffix);
    assert_eq!(
        serde_json::to_value(losses).unwrap(),
        serde_json::to_value(expected_losses).unwrap()
    );
    let (_, native, fidelity, losses) = output.into_parts();
    assert!(native.namespace("iges").is_none());
    assert_eq!(fidelity.retained_records().len(), 1);
    let (_, failed) = fidelity.retained_records().first_key_value().unwrap();
    assert!(!failed.untyped().is_empty());
    let start = fields[..23]
        .iter()
        .map(|field| field.len() + 1)
        .sum::<usize>();
    assert_eq!(failed.offset(), u64::try_from(start).unwrap());
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert!(losses[0].message.starts_with("part iges.global_field "));
}

#[test]
fn detached_source_and_fatal_session_budget_never_fabricate_global_parts() {
    let bytes = file(&fields());
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let scan = card::scan_source(root).unwrap();
    ctx.retain_bytes(ctx.retained_bytes_available(), "consume caller allowance")
        .unwrap();
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    assert!(matches!(
        parse(&scan, &ctx, &mut output),
        Err(CodecError::ResourceLimit(_))
    ));
    assert!(output.native().namespace("iges").is_none());
    assert!(output.fidelity().retained_records().is_empty());
    assert!(output.losses().is_empty());
}
