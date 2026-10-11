// SPDX-License-Identifier: Apache-2.0

use super::{feature_draft_construction_graph_payloads, FeatureDraftConstructionReference};
use cadmpeg_core::decode::{u64_from_index, ResourceDimension};
use cadmpeg_core::CodecError;

fn draft_lane() -> super::FeatureDraftConstructionIndexLane {
    serde_json::from_str(
        r#"{"id":"nx:feature-history:draft-construction-index-lane#key","operation_label":"nx:feature-history:operation-label#key","declared_count":3,"indices":[1,2],"raw_indices":[[1],[2]],"data_blocks":["nx:om-data-blocks-0:block#1","nx:om-data-blocks-0:block#2"],"source_offsets":[24,25]}"#,
    )
    .unwrap()
}

fn draft_references() -> Vec<FeatureDraftConstructionReference> {
    (0..4)
        .map(|ordinal| {
            serde_json::from_value(serde_json::json!({
                "id": format!("reference#{ordinal}"),
                "operation_label": "nx:feature-history:operation-label#key",
                "ordinal": ordinal,
                "object_index": 1,
                "raw_object_index": [240, 1],
                "data_block": "nx:om-data-blocks-0:block#1",
                "source_offset": 100 + ordinal,
            }))
            .unwrap()
        })
        .collect()
}

fn draft_file() -> Vec<u8> {
    let part =
        crate::test_support::test_om::composed_feature_history_payload(&[], &[b"abc", b"def"]);
    crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)])
}

#[test]
fn draft_graph_output_growth_excludes_consumed_source_slots() {
    let references = draft_references();
    let lane = draft_lane();
    let file = draft_file();
    let container = crate::test_support::with_decode_context(|ctx| {
        let container = crate::container::scan_bytes(ctx, &file)?;
        super::offset_data_block_bytes(ctx, &container)?;
        Ok::<_, CodecError>(container)
    })
    .unwrap();
    let groups = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        "test draft group scratch checkpoint",
        |ctx| {
            let (groups, _storage) = ctx.collect_scoped_btree_groups(
                references
                    .iter()
                    .map(|reference| (reference.operation_label.as_str(), reference)),
                "test draft reference groups",
            )?;
            ctx.reserve_scoped(65_536, "test draft group scratch checkpoint")?;
            Ok(groups)
        },
    );
    let CodecError::ResourceLimit(groups) = groups else {
        panic!("group checkpoint refusal")
    };
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_draft_construction_graph_payloads(
            ctx,
            &container,
            std::slice::from_ref(&lane),
            &references,
        )
    };
    let rows = crate::test_support::with_decode_context(decode).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].content.blocks().len(), 4);
    let expected = rows.capacity()
        * std::mem::size_of::<super::FeatureDraftConstructionGraphPayload>()
        + rows[0].id.len()
        + rows[0].operation_label.len()
        + rows[0].index_lane.len()
        + rows[0]
            .construction_references
            .iter()
            .map(String::len)
            .sum::<usize>()
        + rows[0]
            .content
            .blocks()
            .iter()
            .map(|block| block.id.len())
            .sum::<usize>()
        + 64;
    let children_before_growth = expected
        - rows.capacity() * std::mem::size_of::<super::FeatureDraftConstructionGraphPayload>()
        - (rows[0].operation_label.len() + rows[0].index_lane.len());
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        "NX draft construction graph payloads",
        |ctx| {
            let (rows, _storage) =
                ctx.with_scoped_storage("test draft output candidate", || decode(ctx))?;
            Ok(rows)
        },
    );
    // Group scratch and surviving child strings precede output-vector growth.
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.used == groups.used + u64_from_index(children_before_growth)
            && limit.additional == u64_from_index(4 * std::mem::size_of::<super::FeatureDraftConstructionGraphPayload>())));
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "test draft graph retained checkpoint",
        |ctx| {
            let rows = decode(ctx)?;
            ctx.charge_retained(1, "test draft graph retained checkpoint")?;
            Ok(rows)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.used == u64_from_index(expected) && limit.additional == 1));
}

#[test]
fn draft_graph_owner_rejection_releases_source_copies() {
    let mut references = draft_references();
    references[3].data_block = Some("nx:om-data-blocks-1:block#1".into());
    let lane = draft_lane();
    let file = draft_file();
    let container = crate::test_support::with_decode_context(|ctx| {
        let container = crate::container::scan_bytes(ctx, &file)?;
        super::offset_data_block_bytes(ctx, &container)?;
        Ok::<_, CodecError>(container)
    })
    .unwrap();
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_retained_bytes = 0,
        |ctx| {
            let rows =
                feature_draft_construction_graph_payloads(ctx, &container, &[lane], &references)?;
            assert!(rows.is_empty());
            assert_eq!(ctx.resource_refusal(), None);
            Ok::<_, CodecError>(())
        },
    )
    .unwrap();
}
