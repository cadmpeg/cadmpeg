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
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let scan = card::scan_source(root).expect("the fixture scans into cards");
    let (expected, expected_losses) =
        crate::global::parse(&scan).expect("the partless global parse succeeds");
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let (actual, losses) =
        parse(&scan, &ctx, &mut output).expect("the part callbacks parse the global section");
    assert_eq!(actual.sender_product(), Some("product"));
    assert_eq!(actual.native_file_name(), Some("part.igs"));
    assert_eq!(actual.units_name(), Some("MM"));
    assert_eq!(actual.declared_version_flag(), 11);
    assert_eq!(actual.summary_notes(), expected.summary_notes());
    assert_eq!(
        serde_json::to_value(losses).expect("the part losses serialize"),
        serde_json::to_value(expected_losses).expect("the partless losses serialize")
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
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the prefixed fixture fits the service policy");
    let source = root
        .child(3, bytes.len() - 4)
        .expect("the source window lies inside the root");
    let scan = card::scan_source(source).expect("the source window scans into cards");
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let (global, _) =
        parse(&scan, &ctx, &mut output).expect("the part callbacks parse the global section");
    assert_eq!(
        global.sender_product(),
        Some(std::str::from_utf8(&payload).expect("the payload is ascii"))
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
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let scan = card::scan_source(root).expect("the fixture scans into cards");
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let (global, _) = parse(&scan, &ctx, &mut output)
        .expect("an inadmissible descriptive field does not refuse the global section");
    assert_eq!(global.sender_product(), None);
    assert_eq!(global.native_file_name(), Some("part.igs"));
}

#[test]
fn mandatory_global_failure_has_one_shared_actual_source_home_and_no_reads() {
    let bytes = fixed_ascii_with_global(b"X;");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let scan = card::scan_source(root).expect("the fixture scans into cards");
    let original_error = crate::global::parse(&scan)
        .expect_err("the partless parse refuses an undelimited global section");
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    // #222 C.5: the delimiter declarations frame the section, so the section
    // is given up. Every Global value is absent and the delimiters are the
    // IGES 5.3 §2.2.4.3.1-2 defaults.
    let (global, _) = parse(&scan, &ctx, &mut output)
        .expect("a given-up global section does not refuse the part parse");
    assert_eq!(global.parameter_delimiter, b',');
    assert_eq!(global.record_delimiter, b';');
    assert_eq!(global.sender_product(), None);
    assert_eq!(global.native_file_name(), None);
    let (_, native, fidelity, losses) = output.into_parts();
    // #215 rule 1: the given-up part keeps one unknowns home.
    let unknowns = &native
        .namespace("iges")
        .expect("one unknowns home")
        .arenas()["unknowns"];
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        unknowns[0].id(),
        fidelity
            .retained_records()
            .first_key_value()
            .expect("the global section is retained")
            .0
            .as_str()
    );
    let global = scan
        .section(Section::Global)
        .next()
        .expect("the fixture has a global card")
        .1;
    let start = usize::try_from(global.offset).expect("the global card offset fits usize");
    assert_eq!(fidelity.retained_records().len(), 1);
    let (_, section) = fidelity
        .retained_records()
        .first_key_value()
        .expect("the global section is retained");
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
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let scan = card::scan_source(root).expect("the fixture scans into cards");
    let (expected, expected_losses) =
        crate::global::parse(&scan).expect("the partless parse keeps an unreadable suffix");
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let (actual, losses) = parse(&scan, &ctx, &mut output)
        .expect("an unreadable suffix does not refuse the part parse");
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
        .expect("the partless parse reports the unreadable suffix framing loss");
    let mut expected_losses = expected_losses;
    expected_losses.remove(suffix);
    assert_eq!(
        serde_json::to_value(losses).expect("the part losses serialize"),
        serde_json::to_value(expected_losses).expect("the partless losses serialize")
    );
    let (_, native, fidelity, losses) = output.into_parts();
    // #215 rule 1: the given-up part keeps one unknowns home.
    let unknowns = &native
        .namespace("iges")
        .expect("one unknowns home")
        .arenas()["unknowns"];
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        unknowns[0].id(),
        fidelity
            .retained_records()
            .first_key_value()
            .expect("the unreadable field is retained")
            .0
            .as_str()
    );
    assert_eq!(fidelity.retained_records().len(), 1);
    let (_, failed) = fidelity
        .retained_records()
        .first_key_value()
        .expect("the unreadable field is retained");
    assert!(!failed.untyped().is_empty());
    let start = fields[..23]
        .iter()
        .map(|field| field.len() + 1)
        .sum::<usize>();
    assert_eq!(
        failed.offset(),
        u64::try_from(start).expect("the field offset fits u64")
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert!(losses[0].message.starts_with("part iges.global_field "));
}

#[test]
fn detached_source_and_fatal_session_budget_never_fabricate_global_parts() {
    let bytes = file(&fields());
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let scan = card::scan_source(root).expect("the fixture scans into cards");
    ctx.retain_bytes(ctx.retained_bytes_available(), "consume caller allowance")
        .expect("the caller allowance can be consumed in full");
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    assert!(matches!(
        parse(&scan, &ctx, &mut output),
        Err(CodecError::ResourceLimit(_))
    ));
    assert!(output.native().namespace("iges").is_none());
    assert!(output.fidelity().retained_records().is_empty());
    assert!(output.losses().is_empty());
}
