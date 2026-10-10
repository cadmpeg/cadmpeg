// SPDX-License-Identifier: Apache-2.0

use super::operation_identity::label;
use crate::native::features::feature_operation_state_journal_uses;
use crate::native::features::operation_record::FeatureOperationRecord;
use crate::native::features::FeatureOperationLabel;
use crate::native::features::FeatureOperationStateJournalUse;
use crate::native::features::FeatureOperationTerminalFrame;
use crate::native::om::journal_group::OmOperationStateJournalGroup;
use crate::om::state_journal::JournalRow;

fn journal_row(state_ordinal: u32, source_offset: u64) -> JournalRow {
    JournalRow::new(
        source_offset,
        1_700_000_000,
        crate::om::state_tagged_value::StateTaggedValue::read_at(
            &[
                0xe0,
                0,
                0,
                0,
                u8::try_from(state_ordinal).expect("fixture value fits u8"),
            ],
            0,
        )
        .unwrap(),
        crate::om::state_index::StateIndexToken::read_at(&[12], 0).unwrap(),
        crate::om::state_index::StateIndexToken::read_at(
            &[u8::try_from(state_ordinal).expect("fixture value fits u8")],
            0,
        )
        .unwrap(),
    )
    .unwrap()
}

fn journal_group(
    id: &str,
    section_link: &str,
    rows: Vec<JournalRow>,
) -> OmOperationStateJournalGroup {
    let source_offset = rows[0].offset() - 4;
    OmOperationStateJournalGroup {
        id: id.to_string(),
        section_link: section_link.to_string(),
        ordinal: 0,
        frame: crate::om::journal_group::JournalGroup::new([4, 0], source_offset, rows).unwrap(),
        source_entry: "/Root/UG_PART/UG_PART".to_string(),
    }
}

fn operation_record(id: &str, operation_label: &str) -> FeatureOperationRecord {
    FeatureOperationRecord {
        id: id.to_string(),
        operation_label: operation_label.to_string(),
        ordinal: 0,
        sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(b"record-sha256"),
        payload_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(b"payload-sha256"),
        stable_identity: None,
        span: crate::native::features::operation_record::OperationRecordSpan::new(400, 404, 8)
            .unwrap(),
    }
}

fn terminal_frame(operation_record: &str, local_ordinal: u32) -> FeatureOperationTerminalFrame {
    FeatureOperationTerminalFrame {
        id: "nx:feature-history:operation-terminal-frame#0000000000-0000000000".to_string(),
        operation_record: operation_record.to_string(),
        immediate_common_frame: None,
        frame: crate::om::common_frame::TerminalFrame::<u64, Option<String>>::new(
            crate::om::common_frame::CommonFrameSuffix::from_wire(
                local_ordinal,
                &[u8::try_from(local_ordinal).expect("fixture value fits u8")],
                None,
                &[0xff],
            )
            .unwrap()
            .with_target(None)
            .unwrap(),
            420,
        )
        .unwrap(),
    }
}

fn state_journal_uses_for_test(
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    terminal_frames: &[FeatureOperationTerminalFrame],
    groups: &[OmOperationStateJournalGroup],
) -> Vec<FeatureOperationStateJournalUse> {
    crate::test_support::with_decode_context(|ctx| {
        feature_operation_state_journal_uses(ctx, labels, records, terminal_frames, groups)
    })
    .expect("admitted operation state journal uses")
}

fn state_journal_use_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let label = label(0, [None; 4]);
    let record = operation_record(
        "nx:feature-history:operation-record#0000000000-0000000000",
        &label.id,
    );
    let group = journal_group(
        "nx:feature-history:operation-state-journal-group#0000000000-0000000000",
        &label.section_link,
        vec![journal_row(7, 520)],
    );
    let frame = terminal_frame(&record.id, 7);
    let route = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_operation_state_journal_uses(
            ctx,
            std::slice::from_ref(&label),
            std::slice::from_ref(&record),
            std::slice::from_ref(&frame),
            std::slice::from_ref(&group),
        )
    };
    let admitted = crate::test_support::with_decode_context(|ctx| route(ctx))
        .expect("admitted operation journal use");
    assert_eq!(admitted.len(), 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| route(ctx).expect_err("operation journal use resource limit"),
    )
}

#[test]
fn state_journal_use_route_refuses_collection_limit() {
    let error = state_journal_use_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn state_journal_use_route_refuses_retained_limit() {
    let error = state_journal_use_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn state_journal_use_route_refuses_work_limit() {
    let error = state_journal_use_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn operation_terminal_ordinal_joins_unique_section_journal_row() {
    let label = label(0, [None; 4]);
    let record = operation_record(
        "nx:feature-history:operation-record#0000000000-0000000000",
        &label.id,
    );
    let group = journal_group(
        "nx:feature-history:operation-state-journal-group#0000000000-0000000000",
        &label.section_link,
        vec![journal_row(6, 507), journal_row(7, 520)],
    );
    let frame = terminal_frame(&record.id, 7);

    let uses = state_journal_uses_for_test(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        std::slice::from_ref(&frame),
        std::slice::from_ref(&group),
    );

    let [relation] = uses.as_slice() else {
        panic!("one unique section-scoped journal row should join");
    };
    assert_eq!(
        relation.id,
        "nx:feature-history:operation-state-journal-use#0000000000-0000000000-0000000000-0000000000-0000000001"
    );
    assert_eq!(relation.operation_record, record.id);
    assert_eq!(relation.journal_row_ordinal, 1);
    assert_eq!(relation.state_ordinal, 7);
    assert_eq!(relation.operation_source_offset, 420);
    assert_eq!(relation.journal_source_offset, 520);
    let wire = serde_json::to_value(relation).unwrap();
    assert_eq!(wire["state_ordinal"], 7);
    let admitted: FeatureOperationStateJournalUse = serde_json::from_value(wire).unwrap();
    assert_eq!(&admitted, relation);
}

#[test]
fn operation_terminal_ordinal_rejects_wrong_section_and_ambiguous_rows() {
    let label = label(0, [None; 4]);
    let record = operation_record(
        "nx:feature-history:operation-record#0000000000-0000000000",
        &label.id,
    );
    let frame = terminal_frame(&record.id, 7);
    let wrong_section = journal_group(
        "nx:feature-history:operation-state-journal-group#wrong",
        "history#1",
        vec![journal_row(7, 600)],
    );
    let matching = journal_group(
        "nx:feature-history:operation-state-journal-group#matching",
        &label.section_link,
        vec![journal_row(7, 620)],
    );
    let duplicate = journal_group(
        "nx:feature-history:operation-state-journal-group#duplicate",
        &label.section_link,
        vec![journal_row(7, 640)],
    );

    let section_scoped = state_journal_uses_for_test(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        std::slice::from_ref(&frame),
        &[wrong_section, matching.clone()],
    );
    assert_eq!(section_scoped.len(), 1);
    assert_eq!(section_scoped[0].journal_source_offset, 620);

    let ambiguous = state_journal_uses_for_test(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        std::slice::from_ref(&frame),
        &[matching, duplicate],
    );
    assert!(ambiguous.is_empty());
}
