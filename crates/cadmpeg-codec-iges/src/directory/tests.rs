// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use crate::directory::{Hierarchy, Subordinate, UseFlag};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::global::GlobalTable;
use crate::loss::IgesLossCode;
use crate::test_support::test_curves_and_surfaces::{direction_file, point_file};
use crate::test_support::test_owned::{owned_test_file, OwnedTestEntity};
use crate::IgesCodec;

use super::{status, SourceStatus};

#[test]
fn directory_summary_refuses_group_and_note_limits_before_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let entries = [crate::test_support::directory_target(1, 116)];
    for (cap, operation) in [
        (0, "iges directory summary groups"),
        (1, "iges directory summary notes"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::summary_notes(&entries, &ctx);
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.used == cap
                    && limit.additional == 1
                    && limit.operation == operation
        ));
    }

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(
        super::summary_notes(&entries, &ctx).unwrap().0,
        ["entities=1", "entity.116.form.0=1"]
    );
}

#[test]
fn directory_summary_steps_source_entries_before_group_work_can_stop() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let entries = (1..=512)
        .map(|sequence| crate::test_support::directory_target(sequence, 116))
        .collect::<Vec<_>>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::summary_notes(&entries, &ctx).expect_err("the first group work exceeds 1");
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("expected work refusal: {error:?}");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "iges directory summary groups");
    assert_eq!(limit.used, 1);
    assert!(limit.additional > 0);
    assert!(matches!(
        ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(actual)) if actual == limit
    ));
}

#[test]
fn directory_summary_releases_census_storage_and_scopes_note_slots() {
    use cadmpeg_core::decode::{
        u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
    };

    let entries = [crate::test_support::directory_target(1, 116)];
    let tree_limit = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "iges directory summary groups",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            super::summary_notes(&entries, &ctx).map(|_| ())
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(tree_limit) = tree_limit else {
        panic!("expected census storage boundary: {tree_limit:?}");
    };
    assert_eq!(tree_limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(tree_limit.operation, "iges directory summary groups");
    let tree_bytes = tree_limit
        .used
        .checked_add(tree_limit.additional)
        .expect("census storage size fits");

    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let (notes, storage) = super::summary_notes(&entries, &ctx).unwrap();
    assert_eq!(notes, ["entities=1", "entity.116.form.0=1"]);
    let note_bytes = u64_from_index(
        notes
            .capacity()
            .checked_mul(std::mem::size_of::<String>())
            .expect("summary vector size fits"),
    );
    let text_bytes = u64_from_index(
        notes
            .iter()
            .try_fold(0_usize, |total, note| total.checked_add(note.len()))
            .expect("summary text size fits"),
    );
    drop(notes);
    drop(storage);
    let total_bytes = tree_bytes
        .checked_add(note_bytes)
        .expect("summary storage size fits");

    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = total_bytes;
    policy.limits.max_retained_bytes = text_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (notes, storage) = super::summary_notes(&entries, &ctx).unwrap();
    let remaining = ctx
        .reserve_scoped(tree_bytes, "test after directory census release")
        .expect("the census guard is dropped when summary returns");
    let error = ctx
        .reserve_scoped(1, "test after live directory summary slots")
        .expect_err("the returned note slots remain live");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.used == total_bytes
                && limit.additional == 1
                && limit.operation == "test after live directory summary slots"
    ));
    drop(remaining);
    drop(notes);
    drop(storage);
    assert!(matches!(
        ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));

    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = total_bytes;
    policy.limits.max_retained_bytes = text_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (notes, storage) = super::summary_notes(&entries, &ctx).unwrap();
    drop(notes);
    drop(storage);
    let released = ctx
        .reserve_scoped(total_bytes, "test after released directory summary slots")
        .expect("dropping the returned vector releases its backing");
    drop(released);
    let retained = ctx
        .charge_retained(1, "test after retained directory summary text")
        .expect_err("dropping the slots does not release note text");
    assert!(matches!(
        retained,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == text_bytes
                && limit.additional == 1
                && limit.operation == "test after retained directory summary text"
    ));
    assert!(matches!(
        ctx.finish_session(),
        Err(cadmpeg_core::CodecError::ResourceLimit(_))
    ));
}

#[test]
fn directory_entity_refuses_entity_limit_before_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let bytes = point_file();
    let scan = crate::test_support::scan(&bytes).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let result = super::parse(&scan, GlobalTable::V5Later, &ctx);
    assert!(matches!(
        result,
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::Entities
                && limit.used == 0
                && limit.additional == 1
    ));

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    assert!(super::parse(&scan, GlobalTable::V5Later, &ctx).is_ok());
}

#[test]
fn directory_fixed_fields_need_only_the_record_step() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00000000",
        parameters: "116,1,2,3,0;".into(),
    }]);
    let scan = crate::test_support::scan(&bytes).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let (entries, quarantined) = super::parse(&scan, GlobalTable::V5Later, &ctx).unwrap();
    assert!(quarantined.is_empty());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].entity_type, 116);
    assert_eq!(entries[0].parameter_start, 1);
    assert_eq!(entries[0].parameter_line_count, 1);
    assert_eq!(entries[0].form, 0);
    ctx.finish_session().unwrap();
}

#[test]
fn subordinate_switch_dependency_bits_follow_the_four_defined_values() {
    for (subordinate, physical, logical) in [
        (0, false, false),
        (1, true, false),
        (2, false, true),
        (3, true, true),
    ] {
        let status = SourceStatus::from_codes([0, subordinate, 0, 0]);
        assert_eq!(status.is_physically_dependent(), physical);
        assert_eq!(status.is_logically_dependent(), logical);
    }
}

#[test]
fn entity_use_flag_range_follows_the_declared_dialect() {
    for (global_table, use_flag, expected) in [
        (GlobalTable::V4_0, 5, true),
        (GlobalTable::V4_0, 6, false),
        (GlobalTable::V5_0, 6, true),
        (GlobalTable::V5_0, 7, false),
    ] {
        let status = SourceStatus::from_codes([0, 0, use_flag, 0]);
        assert_eq!(status.use_flag(global_table).is_some(), expected);
    }
}

#[test]
fn early_dialects_left_pad_right_justified_status_numbers() {
    for global_table in [GlobalTable::Legacy, GlobalTable::V4_0, GlobalTable::V5_0] {
        let status = status(*b"     201", global_table).unwrap();
        assert!(status.is_visible());
        assert_eq!(status.subordinate(), Some(Subordinate::Independent));
        assert_eq!(status.use_flag(global_table), Some(UseFlag::Definition));
        assert_eq!(status.hierarchy(), Some(Hierarchy::GlobalDefer));
    }

    assert!(status(*b"     201", GlobalTable::V5Later).is_err());
}

#[test]
fn blank_directory_status_defaults_to_zero_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "BLANK".into(),
                status: "        ",
                parameters: "116,1,2,3,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn eight_digit_directory_status_supplies_four_two_digit_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "STATUS".into(),
                status: "01020304",
                parameters: "116,1,2,3,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let entity = &result.ir().native.namespace("iges").unwrap().arenas()["entities"][0];

    assert_eq!(entity.fields()["blank_status"], 1);
    assert_eq!(entity.fields()["subordinate_status"], 2);
    assert_eq!(entity.fields()["use_flag"], 3);
    assert_eq!(entity.fields()["hierarchy_status"], 4);
}

#[test]
fn a_nonblank_space_in_the_status_number_quarantines_the_record() {
    for status in ["     201", "0000 201", "0000020 "] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "STATUS".into(),
                    status,
                    parameters: "116,1,2,3,0;".into(),
                }])),
                &DecodeOptions::default(),
            )
            .unwrap();

        let native = result.ir().native.namespace("iges").unwrap();
        let quarantined = &native.arenas()["quarantined_directory_records"];
        let losses = &result.report().losses;
        assert!(native.arenas()["entities"].is_empty(), "{status}");
        assert_eq!(quarantined.len(), 1, "{status}");
        assert_eq!(quarantined[0].fields()["defect"], "status-number-invalid");
        assert_eq!(losses.len(), 1, "{status}: {losses:#?}");
        assert_eq!(
            losses[0].code,
            IgesLossCode::DirectoryRecordQuarantined.kind()
        );
    }
}

#[test]
fn inspect_reports_directory_entity_and_form_census() {
    let bytes = point_file();

    let summary = IgesCodec
        .inspect(
            &mut Cursor::new(bytes),
            &cadmpeg_core::decode::InspectOptions::default(),
        )
        .unwrap();

    assert!(summary.notes.contains(&"entities=1".into()));
    assert!(summary.notes.contains(&"entity.116.form.0=1".into()));
    assert!(summary.notes.contains(&"parameter_records=1".into()));
    assert!(summary.notes.contains(&"parameter_tokens=4".into()));
}

#[test]
fn decode_treats_subordinate_switch_three_as_physically_dependent() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(direction_file()),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result.report().geometry_transferred());
    assert_eq!(result.report().losses.len(), 1);
    let loss = &result.report().losses[0];
    assert_eq!(loss.code, IgesLossCode::EntityRetainedUnprojected.kind());
    assert_eq!(
        loss.provenance
            .as_ref()
            .and_then(|provenance| provenance.tag.as_deref()),
        Some("directory_entry:D1")
    );
    let native = result.ir().native.namespace("iges").unwrap();
    assert_eq!(native.arenas()["directions"].len(), 1);
    let direction_fields = native.arenas()["directions"][0].fields();
    let components = direction_fields["components"].as_array().unwrap();
    assert_eq!(components[0], 2.0);
    assert_eq!(components[1], -3.0);
    assert_eq!(components[2], 4.0);
    assert_eq!(
        native.arenas()["directions"][0].fields()["physically_dependent"],
        true
    );
    let validation = cadmpeg_ir::validate_neutral(result.ir(), Vec::new())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{:#?}", validation.findings);
}

#[test]
fn residual_status_fields_preserve_numeric_wire_values() {
    for global_table in [GlobalTable::V4_0, GlobalTable::V5Later] {
        let parsed = status(*b"99999999", global_table).unwrap();
        assert!(parsed.use_flag(global_table).is_none());
        assert!(!parsed.is_physically_dependent());
        assert!(!parsed.is_logically_dependent());
        assert_eq!(
            serde_json::to_value(parsed).unwrap(),
            serde_json::json!({
                "blank_status": 99,
                "subordinate_status": 99,
                "use_flag": 99,
                "hierarchy_status": 99,
            })
        );
    }
    let early = status(*b"00000600", GlobalTable::V4_0).unwrap();
    let later = status(*b"00000600", GlobalTable::V5Later).unwrap();
    assert_eq!(early, later);
    assert!(early.use_flag(GlobalTable::V4_0).is_none());
    assert_eq!(
        later.use_flag(GlobalTable::V5Later),
        Some(UseFlag::Construction)
    );
    assert_eq!(
        serde_json::to_value(early).unwrap(),
        serde_json::to_value(later).unwrap()
    );
}
