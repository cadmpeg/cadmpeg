// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::report::loss::LossNote;

use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};

#[test]
fn quarantine_loss_slots_are_scoped_and_payloads_remain_retained() {
    const MATERIALIZED_LIMIT: u64 = 8192;
    for (status, parameters, code, text_operation, tag) in [
        ("0000 201", "116,1,2,3,0;",
            crate::loss::IgesLossCode::DirectoryRecordQuarantined,
            "iges directory quarantine loss message", "directory_entry:D1"),
        ("00000000", "116,1,2,3x4,0;",
            crate::loss::IgesLossCode::ParameterDataQuarantined,
            "iges parameter quarantine loss message", "D1:parameter"),
    ] {
        let bytes = owned_test_file(&[OwnedTestEntity {
            entity_type: 116, form: 0, label: "POINT".into(), status,
            parameters: parameters.into(),
        }]);
        let arena = DecodeArena::new();
        let (parse_ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena,
            &DecodePolicy::service()).unwrap();
        let parse = crate::reader::PhysicalParse::run(&bytes, &parse_ctx,
            crate::reader::ParseMode::Inspect).unwrap();
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "iges record loss slots",
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                parse.record_losses(&ctx).map(|_| ())
            },
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = parse.record_losses(&ctx).unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == text_operation));
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = MATERIALIZED_LIMIT;
        for refuse_while_live in [true, false] {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let (losses, storage) = parse.record_losses(&ctx).unwrap();
            assert_eq!(losses.len(), 1);
            assert_eq!(losses[0].code, code.kind());
            assert_eq!(losses[0].provenance.as_ref().unwrap().tag.as_deref(),
                Some(tag));
            if refuse_while_live {
                let error = ctx.reserve_scoped(MATERIALIZED_LIMIT, "live quarantine loss slots").unwrap_err();
                let cadmpeg_core::CodecError::ResourceLimit(expected) = error else {
                    panic!("expected quarantine loss storage refusal");
                };
                assert_eq!(expected.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(expected.operation, "live quarantine loss slots");
                assert_eq!(expected.used, 4 * u64::try_from(std::mem::size_of::<LossNote>()).unwrap());
                drop(losses);
                drop(storage);
                assert!(matches!(ctx.finish_session().unwrap_err(),
                    cadmpeg_core::CodecError::ResourceLimit(actual) if actual == expected));
            } else {
                drop(losses);
                drop(storage);
                ctx.reserve_scoped(MATERIALIZED_LIMIT, "released quarantine loss slots").unwrap();
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[test]
fn quarantine_loss_payload_refusal_does_not_precharge_unvisited_records() {
    for (status, parameters, visited_steps, operation) in [
        ("0000 201", "116,1,2,3,0;", 1,
            "iges directory quarantine loss message"),
        ("00000000", "116,1,2,3x4,0;", 2,
            "iges parameter quarantine loss message"),
    ] {
        let entities = (0..17).map(|_| OwnedTestEntity {
            entity_type: 116, form: 0, label: "POINT".into(), status,
            parameters: parameters.into(),
        }).collect::<Vec<_>>();
        let bytes = owned_test_file(&entities);
        let arena = DecodeArena::new();
        let (parse_ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena,
            &DecodePolicy::service()).unwrap();
        let parse = crate::reader::PhysicalParse::run(&bytes, &parse_ctx,
            crate::reader::ParseMode::Inspect).unwrap();
        let mut policy = DecodePolicy::service();
        // Parameter losses first inspect the empty Directory quarantine iterator.
        policy.limits.max_work_units = visited_steps;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = parse.record_losses(&ctx).unwrap_err();
        let cadmpeg_core::CodecError::ResourceLimit(expected) = error else {
            panic!("expected quarantine loss text refusal");
        };
        // The first payload formatting step exceeds the visited-prefix budget.
        // A whole-source admission would refuse at the record traversal instead.
        assert_eq!(expected.dimension, ResourceDimension::WorkUnits);
        assert_eq!(expected.operation, operation);
        assert_eq!(expected.used, visited_steps);
        assert!(matches!(ctx.finish_session().unwrap_err(),
            cadmpeg_core::CodecError::ResourceLimit(actual) if actual == expected));
    }
}

#[test]
fn appending_summary_notes_releases_consumed_source_slots() {
    const MATERIALIZED_LIMIT: u64 = 8192;
    const NOTE_COUNT: usize = 17;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = MATERIALIZED_LIMIT;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (mut source, storage) = ctx.temporary_vec(NOTE_COUNT, "test source summary slots").unwrap();
    for _ in 0..NOTE_COUNT {
        source.push(ctx.format_retained(format_args!("a"), "test summary text").unwrap());
    }
    let remaining = ctx.reserve_scoped(MATERIALIZED_LIMIT -
        u64::try_from(NOTE_COUNT * std::mem::size_of::<String>()).unwrap(),
        "remaining storage with source summary").unwrap();
    drop(remaining);
    let mut notes = Vec::new();
    crate::reader::append_summary_notes(&ctx, &mut notes, (source, storage)).unwrap();
    assert_eq!(notes, vec!["a"; NOTE_COUNT]);
    ctx.reserve_scoped(MATERIALIZED_LIMIT, "released source summary slots").unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn parsed_directory_backing_is_scratch_before_native_serialization() {
    use cadmpeg_core::decode::refusal_probe::RefusalProbe;
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    use std::io::Cursor;

    for (status, operation) in [
        ("00000000", "iges directory entries"),
        ("0000 201", "iges quarantined directory entries"),
        ("0000 201", "iges quarantined directory bytes"),
    ] {
        let bytes = owned_test_file(&[OwnedTestEntity {
            entity_type: 116, form: 0, label: "POINT".into(), status,
            parameters: "116,1,2,3,0;".into(),
        }]);
        let probe = RefusalProbe::arm(ResourceDimension::RetainedBytes, operation, None);
        let result = crate::IgesCodec.decode(&mut Cursor::new(&bytes),
            &DecodeOptions::default()).unwrap();
        drop(probe);
        if status == "00000000" {
            assert_eq!(result.ir().model.points.len(), 1);
            assert!(result.report().losses.is_empty());
        } else {
            assert!(result.ir().model.points.is_empty());
            assert!(result.report().losses.iter().any(|loss|
                loss.code == crate::loss::IgesLossCode::DirectoryRecordQuarantined.kind()));
        }
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes, operation, |cap| {
                let mut options = DecodeOptions::default();
                options.policy.limits.max_materialized_bytes = cap;
                crate::IgesCodec.decode(&mut Cursor::new(&bytes), &options)
                    .map(|_| ()).map_err(|failure| match failure {
                        cadmpeg_ir::DecodeFailure::Codec(error) => error,
                        other => panic!("unexpected decode refusal: {other:?}"),
                    })
            },
        );
    }
}

#[test]
fn admission_losses_release_consumed_global_loss_slots() {
    const MATERIALIZED_LIMIT: u64 = 16 * 1024 * 1024;
    let global = String::from_utf8(crate::test_support::global_with_version_flag("11"))
        .unwrap().replace("0.001", "-1");
    let bytes = crate::test_support::test_curves_and_surfaces::point_file_with_global(
        global.as_bytes(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = MATERIALIZED_LIMIT;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut parse = crate::reader::PhysicalParse::run(&bytes, &ctx,
        crate::reader::ParseMode::Inspect).unwrap();
    assert!(!parse.global_losses.is_empty());
    let losses = parse.admission_losses(&ctx).unwrap();
    assert!(losses.iter().any(|loss|
        loss.code == crate::loss::IgesLossCode::GlobalSemanticContextSubstituted.kind()));
    assert!(parse.global_loss_storage.is_none());
    assert_eq!(parse.global_losses.capacity(), 0);
    drop(parse);
    ctx.reserve_scoped(MATERIALIZED_LIMIT,
        "released physical parse storage").unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn attribution_refusal_does_not_precharge_unvisited_losses() {
    let losses = (0..17).map(|_| super::tagged_loss("directory_entry:D1"))
        .collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::reader::attributed_sequences(&losses, &ctx).unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(expected) = error else {
        panic!("expected first attribution tag refusal");
    };
    assert_eq!(expected.dimension, ResourceDimension::WorkUnits);
    assert_eq!(expected.operation, "iges attributed loss tag");
    assert_eq!(expected.used, 1);
    assert!(matches!(ctx.finish_session().unwrap_err(),
        cadmpeg_core::CodecError::ResourceLimit(actual) if actual == expected));
}

#[test]
fn generic_loss_refusal_does_not_precharge_unvisited_directory_entries() {
    let directory = (0..17).map(|slot|
        crate::test_support::directory_target(slot * 2 + 1, 116))
        .collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut losses = Vec::new();
    let mut attributed = std::collections::BTreeSet::new();
    let mut storage = ctx.reserve_scoped(0, "test attribution storage").unwrap();
    let error = crate::reader::append_generic_losses(&ctx, &mut losses, &directory,
        &crate::entities::geometry::Projection::default(), &mut attributed,
        crate::global::GlobalTable::V5Later, &mut storage).unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(expected) = error else {
        panic!("expected first generic loss slot refusal");
    };
    assert_eq!(expected.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(expected.operation, "iges generic loss slots");
    drop(storage);
    assert!(matches!(ctx.finish_session().unwrap_err(),
        cadmpeg_core::CodecError::ResourceLimit(actual) if actual == expected));
}
