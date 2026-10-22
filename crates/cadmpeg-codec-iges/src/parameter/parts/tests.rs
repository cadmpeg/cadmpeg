// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, SourceOrigin};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{ParserPartOutput, PartState};
use cadmpeg_ir::NativeUnknownRecord;

use super::assemble;
use crate::card::{self, Section};
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

fn points(first: &str, second: &str) -> Vec<u8> {
    owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "FIRST".into(),
            status: "00000000",
            parameters: first.into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "SECOND".into(),
            status: "00000000",
            parameters: second.into(),
        },
    ])
}

#[test]
fn actual_parameter_callbacks_preserve_original_values_and_independent_source_extents() {
    let bytes = points("116,1,2,3,0;", "116,4,5,6,0;");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let scan = card::scan_source(root, &ctx).unwrap();
    let global = crate::global::parse(&scan).unwrap().0;
    let directory = crate::directory::parse(&scan, global.global_table(), &ctx)
        .unwrap()
        .0;
    let expected = crate::parameter::assemble(&scan, &directory, &[], &global).unwrap();
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let actual = assemble(&scan, &directory, &[], &global, &ctx, &mut output).unwrap();
    assert_eq!(actual.records, expected.records);
    assert_eq!(
        actual.trailing_pointer_analysis,
        expected.trailing_pointer_analysis
    );
    assert_eq!(actual.quarantined, expected.quarantined);
    assert_eq!(
        serde_json::to_value(actual.recoveries.notes()).unwrap(),
        serde_json::to_value(expected.recoveries.notes()).unwrap()
    );
    let (_, _, fidelity, losses) = output.into_parts().into_parts();
    assert!(losses.is_empty());
    let mut reference = Vec::new();
    cadmpeg_test_support::framing::iges::frame_parameters(&ctx, root, &mut reference).unwrap();
    let mut expected: Vec<_> = reference
        .iter()
        .filter(|part| {
            matches!(
                part.level().as_str(),
                "iges.parameter_section" | "iges.parameter_record"
            )
        })
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
    actual.sort_unstable_by_key(|part| (part.0, part.1, part.2));
    expected.sort_unstable_by_key(|part| (part.0, part.1, part.2));
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 3);
    let (parent_id, _) = fidelity
        .parts()
        .iter()
        .find(|(_, part)| part.level().as_str() == "iges.parameter_section")
        .unwrap();
    assert!(fidelity
        .parts()
        .values()
        .filter(|part| part.level().as_str() == "iges.parameter_record")
        .all(|part| part.parent() == Some(parent_id) && part.state() == &PartState::Decoded {}));
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 2);
    assert_eq!(captures[0].id(), "iges:entity:directory#1");
    assert_eq!(captures[1].id(), "iges:entity:directory#3");
    for (capture, (_, card)) in captures.into_iter().zip(scan.section(Section::Parameter)) {
        assert_eq!(
            capture.reads(),
            &std::collections::BTreeMap::from([
                (capture.stream().to_owned(), vec![4..5, 6..7, 8..9, 10..11]),
                (
                    "file".to_owned(),
                    vec![
                        card.offset + 4..card.offset + 5,
                        card.offset + 6..card.offset + 7,
                        card.offset + 8..card.offset + 9,
                        card.offset + 10..card.offset + 11,
                    ],
                ),
            ])
        );
    }
}

#[test]
fn failed_parameter_record_has_one_exact_shared_home_and_preserves_successful_peer() {
    let bytes = points("110,1,2,3,0;", "116,4,5,6,0;");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let scan = card::scan_source(root, &ctx).unwrap();
    let global = crate::global::parse(&scan).unwrap().0;
    let directory = crate::directory::parse(&scan, global.global_table(), &ctx)
        .unwrap()
        .0;
    let expected = crate::parameter::assemble(&scan, &directory, &[], &global).unwrap();
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let actual = assemble(&scan, &directory, &[], &global, &ctx, &mut output).unwrap();
    assert_eq!(actual.records, expected.records);
    assert_eq!(actual.records.len(), 1);
    assert_eq!(actual.records[0].directory_sequence, 3);
    assert!(actual.quarantined.is_empty());
    let (_, native, fidelity, losses) = output.into_parts().into_parts();
    let unknowns = native
        .namespace("iges")
        .unwrap()
        .arena_as::<NativeUnknownRecord>("unknowns")
        .unwrap();
    assert_eq!(unknowns.len(), 1);
    let first = scan.section(Section::Parameter).next().unwrap().1;
    let start = usize::try_from(first.offset).unwrap();
    assert_eq!(
        fidelity
            .retained_record(unknowns[0].id.as_str())
            .unwrap()
            .data(),
        Some(&bytes[start..start + 81])
    );
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(fidelity.parts().len(), 3);
    assert_eq!(
        fidelity.parts()[&unknowns[0].id].state(),
        &PartState::GivenUp {}
    );
    assert_eq!(
        fidelity
            .parts()
            .values()
            .filter(|part| part.state() == &PartState::Decoded {})
            .count(),
        2
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(losses[0].message.contains("entity-type-token-mismatch"));
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "iges:entity:directory#3");
}

#[test]
fn cross_card_hollerith_token_reads_actual_composite_payload_without_headers_or_comments() {
    let mut payload = "a".repeat(80);
    payload.replace_range(61..65, ",;XP");
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 406,
        form: 15,
        label: "TEXT".into(),
        status: "00000000",
        parameters: format!("406,1,80H{payload};"),
    }]);
    let mut wrapped = b"pre".to_vec();
    wrapped.extend_from_slice(&bytes);
    wrapped.extend_from_slice(b"post");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&wrapped, &arena, &DecodePolicy::service()).unwrap();
    let source = root.child(3, wrapped.len() - 4).unwrap();
    let scan = card::scan_source(source, &ctx).unwrap();
    let global = crate::global::parse(&scan).unwrap().0;
    let directory = crate::directory::parse(&scan, global.global_table(), &ctx)
        .unwrap()
        .0;
    let expected = crate::parameter::assemble(&scan, &directory, &[], &global).unwrap();
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let actual = assemble(&scan, &directory, &[], &global, &ctx, &mut output).unwrap();
    assert_eq!(actual.records, expected.records);
    assert_eq!(actual.records[0].string(2), Some(payload.as_bytes()));
    let streams = ctx.source_streams().unwrap();
    let composite = streams
        .iter()
        .find(|stream| matches!(stream.origin(), SourceOrigin::Composite { .. }))
        .unwrap();
    let SourceOrigin::Composite { segments } = composite.origin() else {
        unreachable!()
    };
    assert_eq!(segments.len(), 3);
    let first = scan.section(Section::Parameter).next().unwrap().1;
    let physical_start = first.offset + 3;
    assert_eq!(segments[0].extent().start, physical_start);
    assert_eq!(segments[0].extent().end, physical_start + 64);
    assert_eq!(segments[1].extent().start, physical_start + 81);
    assert_eq!(segments[1].extent().end, physical_start + 81 + 64);
    assert_eq!(segments[2].extent().start, physical_start + 162);
    assert_eq!(segments[2].extent().end, physical_start + 162 + 64);
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(
        captures[0].reads(),
        &std::collections::BTreeMap::from([
            (composite.name().to_owned(), vec![4..5, 67..147]),
            (
                "file".to_owned(),
                vec![
                    physical_start + 4..physical_start + 5,
                    physical_start + 81 + 3..physical_start + 81 + 64,
                    physical_start + 162..physical_start + 162 + 19,
                ],
            ),
        ])
    );
}

#[test]
fn bounded_macro_record_uses_real_callback_without_invented_child_or_content_reads() {
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 306,
        form: 0,
        label: "MACRO".into(),
        status: "00000000",
        parameters: "306,MACRO,621,X,Y;LET $S=3Ha;b;ENDM;".into(),
    }]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let scan = card::scan_source(root, &ctx).unwrap();
    let global = crate::global::parse(&scan).unwrap().0;
    let directory = crate::directory::parse(&scan, global.global_table(), &ctx)
        .unwrap()
        .0;
    let expected = crate::parameter::assemble(&scan, &directory, &[], &global).unwrap();
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let actual = assemble(&scan, &directory, &[], &global, &ctx, &mut output).unwrap();
    assert_eq!(actual.records, expected.records);
    assert_eq!(actual.records.len(), 1);
    let (_, _, fidelity, losses) = output.into_parts().into_parts();
    assert_eq!(fidelity.parts().len(), 2);
    assert!(losses.is_empty());
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert!(captures[0].reads().is_empty());
}

#[test]
fn missing_parameter_bytes_never_acquire_a_fabricated_source_part() {
    let bytes = points("116,1,2,3,0;", "");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let scan = card::scan_source(root, &ctx).unwrap();
    let global = crate::global::parse(&scan).unwrap().0;
    let directory = crate::directory::parse(&scan, global.global_table(), &ctx)
        .unwrap()
        .0;
    let expected = crate::parameter::assemble(&scan, &directory, &[], &global).unwrap();
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let actual = assemble(&scan, &directory, &[], &global, &ctx, &mut output).unwrap();
    assert_eq!(actual.records, expected.records);
    assert_eq!(actual.records.len(), 1);
    assert_eq!(actual.records[0].directory_sequence, 1);
    let (_, native, fidelity, losses) = output.into_parts().into_parts();
    assert!(native.namespace("iges").is_none());
    assert_eq!(fidelity.parts().len(), 2);
    assert!(fidelity.parts().values().all(|part| part.byte_len() > 0));
    assert!(losses.is_empty());
}

#[test]
fn detached_scan_and_session_budget_refusal_create_no_parameter_failure_home() {
    let bytes = points("116,1,2,3,0;", "116,4,5,6,0;");
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let detached = card::scan(&bytes).unwrap();
    let global = crate::global::parse(&detached).unwrap().0;
    let directory = crate::directory::parse(&detached, global.global_table(), &ctx)
        .unwrap()
        .0;
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    assert!(assemble(&detached, &directory, &[], &global, &ctx, &mut output).is_err());
    let scan = card::scan_source(root, &ctx).unwrap();
    ctx.retain_bytes(ctx.retained_bytes_available(), "consume caller allowance")
        .unwrap();
    assert!(matches!(
        assemble(&scan, &directory, &[], &global, &ctx, &mut output),
        Err(CodecError::ResourceLimit(_))
    ));
    let (_, native, fidelity, losses) = output.into_parts().into_parts();
    assert!(native.namespace("iges").is_none());
    assert!(fidelity.parts().is_empty());
    assert!(losses.is_empty());
}
