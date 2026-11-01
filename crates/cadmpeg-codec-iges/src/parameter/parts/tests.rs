// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::PartOutput;

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
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let scan = card::scan_source(root).expect("the fixture scans into cards");
    let global = crate::global::parse(&scan)
        .expect("the global section parses")
        .0;
    let directory = crate::directory::parse(&scan, global.global_table(), &ctx)
        .expect("the directory section parses")
        .0;
    let expected = crate::parameter::assemble(&scan, &directory, [], &global)
        .expect("the partless parameter assembly succeeds");
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    let actual = assemble(&scan, &directory, [], &global, &ctx, &mut output)
        .expect("the part callbacks assemble the parameter section");
    assert_eq!(actual.records, expected.records);
    assert!(actual.given_up.is_empty());
    assert_eq!(
        actual.trailing_pointer_analysis,
        expected.trailing_pointer_analysis
    );
    assert_eq!(actual.quarantined, expected.quarantined);
    assert_eq!(
        serde_json::to_value(actual.recoveries.notes()).expect("the part recovery notes serialize"),
        serde_json::to_value(expected.recoveries.notes())
            .expect("the partless recovery notes serialize")
    );
    let (model, _, fidelity, losses) = output.into_parts();
    assert_eq!(model.points.len(), 2);
    assert_eq!(model.vertices.len(), 2);
    for (point, vertex) in model.points.iter().zip(&model.vertices) {
        assert_eq!(vertex.point, point.id);
    }
    assert!(losses.is_empty());
    assert!(fidelity.retained_records().is_empty());
}

#[test]
fn failed_parameter_record_has_one_exact_shared_home_and_preserves_successful_peer() {
    let bytes = points("110,1,2,3,0;", "116,4,5,6,0;");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let scan = card::scan_source(root).expect("the fixture scans into cards");
    let global = crate::global::parse(&scan)
        .expect("the global section parses")
        .0;
    let directory = crate::directory::parse(&scan, global.global_table(), &ctx)
        .expect("the directory section parses")
        .0;
    let expected = crate::parameter::assemble(&scan, &directory, [], &global)
        .expect("the partless parameter assembly keeps the peer");
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    let actual = assemble(&scan, &directory, [], &global, &ctx, &mut output)
        .expect("a mismatched entity type does not refuse the parameter section");
    assert_eq!(actual.records, expected.records);
    assert_eq!(actual.records.len(), 1);
    assert_eq!(actual.records[0].directory_sequence, 3);
    assert!(actual.quarantined.is_empty());
    let (_, native, fidelity, losses) = output.into_parts();
    // #215 rule 1: the given-up part keeps one unknowns home.
    let namespace = native.namespace("iges").expect("one unknowns home");
    assert_eq!(namespace.arenas().len(), 1);
    let unknowns = &namespace.arenas()["unknowns"];
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        unknowns[0].id(),
        fidelity
            .retained_records()
            .keys()
            .next()
            .expect("one retained part")
            .as_str()
    );
    let first = scan
        .section(Section::Parameter)
        .next()
        .expect("the fixture has a parameter card")
        .1;
    let start = usize::try_from(first.offset).expect("the parameter card offset fits usize");
    let parts = crate::test_support::given_up_parts(&fidelity);
    assert_eq!(parts.len(), 1);
    assert!(!parts[0].untyped().is_empty());
    assert_eq!(parts[0].data(), Some(&bytes[start..start + 81]));
    assert_eq!(fidelity.retained_records().len(), 1);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert_eq!(losses[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(losses[0].message.contains("entity-type-token-mismatch"));
    assert!(losses[0].message.starts_with("part iges.parameter_record "));
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
    let (ctx, root) = DecodeContext::from_root_bytes(&wrapped, &arena, &DecodePolicy::service())
        .expect("the prefixed fixture fits the service policy");
    let source = root
        .child(3, wrapped.len() - 4)
        .expect("the source window lies inside the root");
    let scan = card::scan_source(source).expect("the source window scans into cards");
    let global = crate::global::parse(&scan)
        .expect("the global section parses")
        .0;
    let directory = crate::directory::parse(&scan, global.global_table(), &ctx)
        .expect("the directory section parses")
        .0;
    let expected = crate::parameter::assemble(&scan, &directory, [], &global)
        .expect("the partless parameter assembly succeeds");
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    let actual = assemble(&scan, &directory, [], &global, &ctx, &mut output)
        .expect("the part callbacks assemble the parameter section");
    assert_eq!(actual.records, expected.records);
    assert_eq!(actual.records[0].string(2), Some(payload.as_bytes()));
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
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let scan = card::scan_source(root).expect("the fixture scans into cards");
    let global = crate::global::parse(&scan)
        .expect("the global section parses")
        .0;
    let directory = crate::directory::parse(&scan, global.global_table(), &ctx)
        .expect("the directory section parses")
        .0;
    let expected = crate::parameter::assemble(&scan, &directory, [], &global)
        .expect("the partless parameter assembly succeeds");
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    let actual = assemble(&scan, &directory, [], &global, &ctx, &mut output)
        .expect("the part callbacks assemble the macro record");
    assert_eq!(actual.records, expected.records);
    assert_eq!(actual.records.len(), 1);
    let (_, _, fidelity, losses) = output.into_parts();
    assert!(fidelity.retained_records().is_empty());
    assert!(losses.is_empty());
}

#[test]
fn missing_parameter_bytes_never_acquire_a_fabricated_source_part() {
    let bytes = points("116,1,2,3,0;", "");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let scan = card::scan_source(root).expect("the fixture scans into cards");
    let global = crate::global::parse(&scan)
        .expect("the global section parses")
        .0;
    let directory = crate::directory::parse(&scan, global.global_table(), &ctx)
        .expect("the directory section parses")
        .0;
    let expected = crate::parameter::assemble(&scan, &directory, [], &global)
        .expect("the partless parameter assembly keeps the peer");
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    let actual = assemble(&scan, &directory, [], &global, &ctx, &mut output)
        .expect("missing parameter bytes do not refuse the parameter section");
    assert_eq!(actual.records, expected.records);
    assert_eq!(actual.records.len(), 1);
    assert_eq!(actual.records[0].directory_sequence, 1);
    let (_, native, fidelity, losses) = output.into_parts();
    assert!(native.namespace("iges").is_none());
    assert!(fidelity.retained_records().is_empty());
    assert!(losses.is_empty());
}

#[test]
fn detached_scan_and_session_budget_refusal_create_no_parameter_failure_home() {
    let bytes = points("116,1,2,3,0;", "116,4,5,6,0;");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let detached = card::scan(&bytes).expect("the detached fixture scans into cards");
    let global = crate::global::parse(&detached)
        .expect("the global section parses")
        .0;
    let directory = crate::directory::parse(&detached, global.global_table(), &ctx)
        .expect("the directory section parses")
        .0;
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    assert!(assemble(&detached, &directory, [], &global, &ctx, &mut output).is_err());
    let scan = card::scan_source(root).expect("the fixture scans into cards");
    ctx.retain_bytes(ctx.retained_bytes_available(), "consume caller allowance")
        .expect("the caller allowance can be consumed in full");
    assert!(matches!(
        assemble(&scan, &directory, [], &global, &ctx, &mut output),
        Err(CodecError::ResourceLimit(_))
    ));
    assert!(output.native().namespace("iges").is_none());
    assert!(output.fidelity().retained_records().is_empty());
    assert!(output.losses().is_empty());
}

#[test]
fn distinct_conflicted_source_parts_keep_one_home_each_and_a_successful_peer() {
    let mut bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "FIRST".into(),
            status: "00000000",
            parameters: "116,1,2,3,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "SECOND".into(),
            status: "00000000",
            parameters: "116,4,5,6,0;".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "PEER".into(),
            status: "00000000",
            parameters: "116,7,8,9,0;".into(),
        },
    ]);
    let first = bytes
        .chunks_exact(81)
        .position(|card| card[72] == b'P')
        .expect("the fixture has a parameter card")
        * 81;
    bytes[first + 64..first + 72].copy_from_slice(b"       3");
    bytes[first + 81 + 64..first + 81 + 72].copy_from_slice(b"       1");
    let mut wrapped = b"pre".to_vec();
    wrapped.extend_from_slice(&bytes);
    wrapped.extend_from_slice(b"post");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&wrapped, &arena, &DecodePolicy::service())
        .expect("the prefixed fixture fits the service policy");
    let source = root
        .child(3, wrapped.len() - 4)
        .expect("the source window lies inside the root");
    let scan = card::scan_source(source).expect("the source window scans into cards");
    let global = crate::global::parse(&scan)
        .expect("the global section parses")
        .0;
    let directory = crate::directory::parse(&scan, global.global_table(), &ctx)
        .expect("the directory section parses")
        .0;
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    let actual = assemble(&scan, &directory, [], &global, &ctx, &mut output)
        .expect("conflicted records do not refuse the parameter section");
    assert_eq!(actual.given_up, std::collections::BTreeSet::from([1, 3]));
    assert_eq!(actual.records.len(), 1);
    assert_eq!(actual.records[0].directory_sequence, 5);
    assert_eq!(actual.records[0].integer(1), Some(7));
    assert!(actual.quarantined.is_empty());
    let (_, native, fidelity, losses) = output.into_parts();
    // #215 rule 1: each given-up part keeps one unknowns home.
    let namespace = native.namespace("iges").expect("one unknowns home");
    assert_eq!(namespace.arenas().len(), 1);
    let unknowns = &namespace.arenas()["unknowns"];
    assert_eq!(unknowns.len(), 2);
    let mut stubbed: Vec<_> = unknowns
        .iter()
        .map(cadmpeg_ir::native::NativeRecord::id)
        .collect();
    stubbed.sort_unstable();
    let mut retained: Vec<_> = fidelity
        .retained_records()
        .keys()
        .map(cadmpeg_ir::ids::UnknownId::as_str)
        .collect();
    retained.sort_unstable();
    assert_eq!(stubbed, retained);
    let mut parts = crate::test_support::given_up_parts(&fidelity);
    parts.sort_unstable_by_key(|part| part.offset());
    assert_eq!(parts.len(), 2);
    for (ordinal, part) in parts.into_iter().enumerate() {
        let start = first + ordinal * 81 + 3;
        assert!(!part.untyped().is_empty());
        assert_eq!(
            part.offset(),
            u64::try_from(start).expect("the part offset fits u64")
        );
        assert_eq!(part.byte_len(), 81);
        assert_eq!(part.data(), Some(&wrapped[start..start + 81]));
    }
    assert_eq!(losses.len(), 2);
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() == "record.unreadable"
            && loss.severity == cadmpeg_ir::report::Severity::Error));
}

#[test]
fn unreadable_point_coordinates_give_up_the_actual_part_and_keep_the_semantic_peer() {
    let bytes = points("116,1,2,broken,0;", "116,4,5,6,0;");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let scan = card::scan_source(root).expect("the fixture scans into cards");
    let global = crate::global::parse(&scan)
        .expect("the global section parses")
        .0;
    let directory = crate::directory::parse(&scan, global.global_table(), &ctx)
        .expect("the directory section parses")
        .0;
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    let records = assemble(&scan, &directory, [], &global, &ctx, &mut output)
        .expect("an unreadable coordinate does not refuse the parameter section");
    assert!(records.given_up.is_empty());
    assert_eq!(records.records.len(), 2);
    assert_eq!(records.records[0].directory_sequence, 1);
    assert!(records.records[0].tokens()[3].is_none());
    assert_eq!(records.records[0].integer(1), Some(1));
    assert_eq!(records.records[0].integer(2), Some(2));
    assert_eq!(records.records[0].integer(4), Some(0));
    assert_eq!(records.records[1].directory_sequence, 3);
    let (model, native, fidelity, losses) = output.into_parts();
    assert_eq!(model.points.len(), 1);
    assert_eq!(model.vertices.len(), 1);
    assert_eq!(
        model.points[0].position().get(),
        cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0)
    );
    assert_eq!(model.vertices[0].point, model.points[0].id);
    // #215 rule 1: the given-up part keeps one unknowns home.
    let namespace = native.namespace("iges").expect("one unknowns home");
    assert_eq!(namespace.arenas().len(), 1);
    let unknowns = &namespace.arenas()["unknowns"];
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        unknowns[0].id(),
        fidelity
            .retained_records()
            .keys()
            .next()
            .expect("one retained part")
            .as_str()
    );
    let parts = crate::test_support::given_up_parts(&fidelity);
    assert_eq!(parts.len(), 1);
    let part = parts[0];
    assert!(!part.untyped().is_empty());
    let start = bytes
        .chunks_exact(81)
        .position(|line| line[72] == b'P')
        .expect("the fixture has a parameter card")
        * 81;
    assert_eq!(part.offset(), 8);
    assert_eq!(part.byte_len(), 7);
    assert_eq!(part.data(), Some(&bytes[start + 8..start + 15]));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
    assert!(losses[0].message.contains("token-not-a-number"));
    assert!(losses[0].message.starts_with("part iges.parameter_field "));
}

#[test]
fn failed_coordinate_field_discards_only_its_actual_composite_and_parent_reads() {
    let original = points("116,1,2,3x4,0;", "116,4,5,6,0;");
    let mut wrapped = b"pre".to_vec();
    wrapped.extend_from_slice(&original);
    wrapped.extend_from_slice(b"post");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&wrapped, &arena, &DecodePolicy::service())
        .expect("the prefixed fixture fits the service policy");
    let source = root
        .child(3, wrapped.len() - 4)
        .expect("the source window lies inside the root");
    let scan = card::scan_source(source).expect("the source window scans into cards");
    let global = crate::global::parse(&scan)
        .expect("the global section parses")
        .0;
    let directory = crate::directory::parse(&scan, global.global_table(), &ctx)
        .expect("the directory section parses")
        .0;
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    let records = assemble(&scan, &directory, [], &global, &ctx, &mut output)
        .expect("an unreadable coordinate does not refuse the parameter section");
    assert!(records.given_up.is_empty());
    assert_eq!(records.records.len(), 2);
    assert!(records.records[0].tokens()[3].is_none());
    let (_, _, fidelity, losses) = output.into_parts();
    assert_eq!(losses.len(), 1);
    let failed = crate::test_support::given_up_parts(&fidelity);
    assert_eq!(failed.len(), 1);
    let failed = failed[0];
    assert!(!failed.untyped().is_empty());
    assert!(losses[0].message.starts_with("part iges.parameter_field "));
    assert_eq!(failed.offset(), 8);
    assert_eq!(failed.byte_len(), 4);
}

#[test]
fn cross_card_numeric_failure_rolls_back_reads_in_each_actual_source_segment() {
    let mut x = "0".repeat(60);
    x.push('1');
    let bytes = crate::test_support::test_owned::owned_test_file_with_raw_parameters(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "SPLIT".into(),
            status: "00010000",
            parameters: format!("116,{x},2,3,0;"),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "PEER".into(),
            status: "00000000",
            parameters: "116,4,5,6,0;".into(),
        },
    ]);
    let mut wrapped = b"pre".to_vec();
    wrapped.extend_from_slice(&bytes);
    wrapped.extend_from_slice(b"post");
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&wrapped, &arena, &DecodePolicy::service())
        .expect("the prefixed fixture fits the service policy");
    let source = root
        .child(3, wrapped.len() - 4)
        .expect("the source window lies inside the root");
    let scan = card::scan_source(source).expect("the source window scans into cards");
    let global = crate::global::parse(&scan)
        .expect("the global section parses")
        .0;
    let directory = crate::directory::parse(&scan, global.global_table(), &ctx)
        .expect("the directory section parses")
        .0;
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges, &ctx);
    let actual = assemble(&scan, &directory, [], &global, &ctx, &mut output)
        .expect("a cross-card numeric failure does not refuse the parameter section");
    assert_eq!(actual.records.len(), 2);
    assert!(actual.given_up.is_empty());
    assert!(actual.quarantined.is_empty());
    assert!(actual.records[0].tokens()[1].is_none());
    assert_eq!(actual.records[0].number(2), Some(2.0));
    assert_eq!(actual.records[0].number(3), Some(3.0));
    assert_eq!(actual.records[0].integer(4), Some(0));
    let (model, _, fidelity, losses) = output.into_parts();
    assert_eq!(model.points.len(), 1);
    assert_eq!(
        model.points[0].position().get(),
        cadmpeg_ir::math::Point3::new(4.0, 5.0, 6.0)
    );
    assert_eq!(losses.len(), 1);
    let failed = crate::test_support::given_up_parts(&fidelity);
    assert_eq!(failed.len(), 1);
    let failed = failed[0];
    assert!(!failed.untyped().is_empty());
    assert!(losses[0].message.starts_with("part iges.parameter_field "));
    assert_eq!(failed.offset(), 4);
    assert_eq!(failed.byte_len(), 62);
}
