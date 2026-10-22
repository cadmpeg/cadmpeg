// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, SourceOrigin, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{ParserPartOutput, PartState};
use cadmpeg_ir::NativeUnknownRecord;

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
    let scan = card::scan_source(root, &ctx).unwrap();
    let (expected, expected_losses) = crate::global::parse(&scan).unwrap();
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
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
    let (_, _, fidelity, losses) = output.into_parts().into_parts();
    assert!(losses.is_empty());
    let mut reference = Vec::new();
    cadmpeg_test_support::framing::iges::frame_global(&ctx, root, &mut reference).unwrap();
    let mut expected: Vec<_> = reference
        .iter()
        .map(|part| {
            (
                part.level().as_str(),
                part.capture().offset(),
                part.capture().byte_len(),
                part.framing().to_vec(),
            )
        })
        .collect();
    let mut actual: Vec<_> = fidelity
        .parts()
        .values()
        .map(|part| {
            (
                part.level().as_str(),
                part.offset(),
                part.byte_len(),
                part.framing_bytes().to_vec(),
            )
        })
        .collect();
    expected.sort_unstable_by_key(|part| (part.0, part.1, part.2));
    actual.sort_unstable_by_key(|part| (part.0, part.1, part.2));
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 25);
    let (parent_id, parent) = fidelity
        .parts()
        .iter()
        .find(|(_, part)| part.level().as_str() == "iges.global_section")
        .unwrap();
    assert_eq!(parent.state(), &PartState::Decoded {});
    assert!(fidelity
        .parts()
        .values()
        .filter(|part| part.level().as_str() == "iges.global_field")
        .all(|part| part.parent() == Some(parent_id)));
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
    let scan = card::scan_source(source, &ctx).unwrap();
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let (global, _) = parse(&scan, &ctx, &mut output).unwrap();
    assert_eq!(
        global.sender_product(),
        Some(std::str::from_utf8(&payload).unwrap())
    );
    assert_eq!(global.native_file_name(), None);
    let streams = ctx.source_streams().unwrap();
    let composite = streams
        .iter()
        .find(|stream| matches!(stream.origin(), SourceOrigin::Composite { .. }))
        .unwrap();
    let SourceOrigin::Composite { segments } = composite.origin() else {
        unreachable!()
    };
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].extent().start, 84);
    assert_eq!(segments[0].extent().end, 156);
    assert_eq!(segments[1].extent().start, 165);
    assert_eq!(segments[1].extent().end, 237);
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 2);
    let metadata = captures
        .iter()
        .find(|capture| capture.offset() == 2)
        .unwrap();
    assert_eq!(metadata.byte_len(), 84);
    assert_eq!(
        metadata.reads(),
        &std::collections::BTreeMap::from([
            (composite.name().to_owned(), vec![5..85]),
            ("file".to_owned(), vec![89..156, 165..178]),
        ])
    );
    assert!(captures
        .iter()
        .find(|capture| capture.offset() == 86)
        .unwrap()
        .reads()
        .is_empty());
    let (_, _, fidelity, losses) = output.into_parts().into_parts();
    assert_eq!(fidelity.parts().len(), 3);
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
    let scan = card::scan_source(root, &ctx).unwrap();
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let (global, _) = parse(&scan, &ctx, &mut output).unwrap();
    assert_eq!(global.sender_product(), None);
    assert_eq!(global.native_file_name(), Some("part.igs"));
    let mut captures = ctx.take_record_captures().unwrap();
    captures.sort_unstable_by_key(|capture| capture.offset());
    assert_eq!(captures.len(), 24);
    for index in [2, 4, 5, 17, 19, 20, 21, 23, 24, 25] {
        assert!(
            captures[index - 2].reads().is_empty(),
            "field {}",
            index + 1
        );
    }
    assert_eq!(
        captures
            .iter()
            .filter(|capture| !capture.reads().is_empty())
            .count(),
        3
    );
}

#[test]
fn mandatory_global_failure_has_one_shared_actual_source_home_and_no_reads() {
    let bytes = fixed_ascii_with_global(b"X;");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let scan = card::scan_source(root, &ctx).unwrap();
    let original_error = crate::global::parse(&scan).unwrap_err();
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let error = parse(&scan, &ctx, &mut output).unwrap_err();
    assert!(error.to_string().contains(&original_error.to_string()));
    let (_, native, fidelity, losses) = output.into_parts().into_parts();
    let unknowns = native
        .namespace("iges")
        .unwrap()
        .arena_as::<NativeUnknownRecord>("unknowns")
        .unwrap();
    assert_eq!(unknowns.len(), 1);
    let global = scan.section(Section::Global).next().unwrap().1;
    let start = usize::try_from(global.offset).unwrap();
    assert_eq!(
        fidelity
            .retained_record(unknowns[0].id.as_str())
            .unwrap()
            .data(),
        Some(&bytes[start..start + 81])
    );
    assert_eq!(fidelity.parts().len(), 1);
    assert_eq!(
        fidelity.parts()[&unknowns[0].id].state(),
        &PartState::GivenUp {}
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn unreadable_descriptive_suffix_keeps_original_preceding_values_and_losses() {
    let mut fields = fields();
    fields[23] = "99999Hx".into();
    let bytes = file(&fields);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let scan = card::scan_source(root, &ctx).unwrap();
    let (expected, expected_losses) = crate::global::parse(&scan).unwrap();
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let (actual, losses) = parse(&scan, &ctx, &mut output).unwrap();
    assert_eq!(actual.sender_product(), Some("product"));
    assert_eq!(actual.declared_version_flag(), 11);
    assert_eq!(actual.summary_notes(), expected.summary_notes());
    assert_eq!(
        serde_json::to_value(losses).unwrap(),
        serde_json::to_value(expected_losses).unwrap()
    );
    let (_, native, fidelity, losses) = output.into_parts().into_parts();
    assert!(native.namespace("iges").is_none());
    assert_eq!(fidelity.parts().len(), 22);
    assert!(fidelity
        .parts()
        .values()
        .all(|part| part.state() == &PartState::Decoded {}));
    assert!(losses.is_empty());
}

#[test]
fn detached_source_and_fatal_session_budget_never_fabricate_global_parts() {
    let bytes = file(&fields());
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    assert!(card::scan_source(View::over_retained(&bytes), &ctx).is_err());
    let scan = card::scan_source(root, &ctx).unwrap();
    ctx.retain_bytes(ctx.retained_bytes_available(), "consume caller allowance")
        .unwrap();
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    assert!(matches!(
        parse(&scan, &ctx, &mut output),
        Err(CodecError::ResourceLimit(_))
    ));
    let (_, native, fidelity, losses) = output.into_parts().into_parts();
    assert!(native.namespace("iges").is_none());
    assert!(fidelity.parts().is_empty());
    assert!(losses.is_empty());
}
