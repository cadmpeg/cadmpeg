// SPDX-License-Identifier: Apache-2.0

use super::super::InstanceJournal;
use super::{
    object_record, scan_with_objects, with_expand, ArchiveVersion, DecodeContext, GeometryOutcome,
    POINT_CLASS,
};

#[test]
fn instance_row_journal_captures_only_selected_rows_once() {
    let objects = (0..128)
        .map(|_| object_record(ArchiveVersion::V5, 1, POINT_CLASS))
        .collect::<Vec<_>>();
    let scan = scan_with_objects(&objects);
    with_expand(&scan, |expand| {
        let mut transaction = DecodeContext::new(&scan, expand).unwrap();
        transaction
            .append_link(9, "rhino:test:curve#original")
            .unwrap();
        transaction.instance_journal = Some(InstanceJournal::new(expand.ctx()).unwrap());
        transaction
            .append_link(9, "rhino:test:curve#later")
            .unwrap();
        transaction.mark_decoded(9);
        transaction
            .append_link(9, "rhino:test:curve#another")
            .unwrap();
        let journal = transaction.instance_journal.as_ref().unwrap();
        assert_eq!(journal.rows.len(), 1);
        let row = &journal.rows[&9];
        assert!(row.sorted_at_checkpoint);
        assert_eq!(
            row.additions,
            std::collections::BTreeSet::from([
                "rhino:test:curve#another".to_owned(),
                "rhino:test:curve#later".to_owned(),
            ])
        );
        assert_eq!(row.status, None);
        assert_eq!(transaction.unknown(9).unwrap().links().len(), 3);
    });
}

#[test]
fn commit_sorts_links_after_indexed_append_and_deduplication() {
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    with_expand(&scan, |expand| {
        let mut transaction = DecodeContext::new(&scan, expand).unwrap();
        transaction
            .append_link(0, "rhino:test:curve#z")
            .unwrap();
        transaction
            .append_links(
                0,
                &[
                    "rhino:test:curve#a".to_string(),
                    "rhino:test:curve#z".to_string(),
                    "rhino:test:curve#m".to_string(),
                ],
            )
            .unwrap();
        let decoded = transaction.commit().unwrap();
        let records = decoded.ir.native_unknowns("rhino").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0]
                .links
                .iter()
                .map(|identity| identity.as_str())
                .collect::<Vec<_>>(),
            ["rhino:test:curve#a", "rhino:test:curve#m", "rhino:test:curve#z"]
        );
    });
}

#[test]
fn instance_row_journal_rolls_back_links_and_statuses_without_replacing_untouched_rows() {
    let objects = (0..3)
        .map(|_| object_record(ArchiveVersion::V5, 1, POINT_CLASS))
        .collect::<Vec<_>>();
    let scan = scan_with_objects(&objects);
    with_expand(&scan, |expand| {
        let mut transaction = DecodeContext::new(&scan, expand).unwrap();
        transaction
            .append_link(0, "rhino:test:curve#original")
            .unwrap();
        transaction.instance_journal = Some(InstanceJournal::new(expand.ctx()).unwrap());
        transaction
            .append_link(0, "rhino:test:curve#later")
            .unwrap();
        transaction.mark_decoded(0);
        transaction.mark_decoded(2);
        transaction.rollback_instance_rows().unwrap();
        assert_eq!(
            transaction.unknown(0).unwrap().links(),
            ["rhino:test:curve#original"]
        );
        assert_eq!(transaction.statuses[0], None);
        assert_eq!(transaction.statuses[2], Some(GeometryOutcome::Decoded));
        assert!(transaction.instance_journal.is_none());
        assert!(expand.ctx().resource_refusal().is_none());
    });
}

#[test]
fn instance_row_journal_preserves_resource_refusal_before_link_mutation() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::MaterializedBytes,
        ResourceDimension::WorkUnits,
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            dimension,
            "Rhino instance touched rows",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                match dimension {
                    ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                    ResourceDimension::MaterializedBytes => {
                        policy.limits.max_materialized_bytes = cap;
                    }
                    ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                    _ => panic!("journal probe dimension"),
                }
                let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                    scan.data, &arena, &policy,
                )?;
                let mut transaction =
                    DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
                transaction.instance_journal = Some(InstanceJournal::new(&ctx)?);
                let result = transaction.append_link(0, "rhino:test:curve#later");
                assert!(transaction.unknown(0).unwrap().links().is_empty());
                if let Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal)) = result {
                    assert_eq!(ctx.resource_refusal(), Some(*refusal));
                }
                result
            },
        );
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
        if refusal.operation == "Rhino instance touched rows" && refusal.dimension == dimension)
        );
    }
}

#[test]
fn instance_row_journal_does_not_copy_existing_link_text() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    let original = (0..64)
        .map(|index| format!("rhino:test:curve#{}-{index:03}", "x".repeat(128)))
        .collect::<Vec<_>>();
    let run = |cap, measure_headroom| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
        let mut transaction = DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
        for link in &original { transaction.append_link(0, link)?; }
        if measure_headroom {
            let headroom = ctx.reserve_scoped(4096, "journal remaining headroom")?;
            drop(headroom);
        }
        transaction.instance_journal = Some(InstanceJournal::new(&ctx)?);
        transaction.append_link(0, "rhino:test:curve#added")
            .expect("only the new link is copied into scratch");
        transaction.mark_decoded(0);
        transaction.rollback_instance_rows().unwrap();
        assert_eq!(transaction.unknown(0).unwrap().links(), original);
        assert_eq!(transaction.statuses[0], None);
        assert!(ctx.resource_refusal().is_none());
        drop(transaction);
        ctx.finish_session()
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes, "journal remaining headroom", |cap| run(cap, true));
    let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else { panic!("headroom probe must refuse"); };
    assert_eq!(refusal.additional, 4096);
    // Setup owns at least 64 * 149 = 9536 text bytes, besides the index nodes.
    // The journal has 4096 bytes beyond that live setup, below a full text copy.
    run(refusal.used + refusal.additional, false).unwrap();
}

#[test]
fn instance_row_journal_field_transfer_preserves_retained_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    let scan = scan_with_objects(&[object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    let link = "rhino:test:curve#added";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "Rhino instance link field text",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, root) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
            let mut transaction =
                DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
            transaction.instance_journal = Some(InstanceJournal::new(&ctx)?);
            transaction.append_link(0, link)?;
            let journal = transaction.instance_journal.take().unwrap();
            let result = journal.field_text_storage.commit();
            if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal(), Some(*refusal));
            }
            result
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else {
        panic!("field text transfer must preserve its retained refusal");
    };
    assert_eq!(refusal.operation, "Rhino instance link field text");
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    // Only the actual new field text transfers; journal keys remain scratch.
    assert_eq!(refusal.additional, u64::try_from(link.len()).unwrap());
}
