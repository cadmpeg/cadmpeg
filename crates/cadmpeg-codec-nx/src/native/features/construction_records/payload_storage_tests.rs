// SPDX-License-Identifier: Apache-2.0

use super::{feature_surface_construction_payloads, FeatureSurfaceConstructionReference};
use cadmpeg_core::decode::{u64_from_index, ResourceDimension};
use cadmpeg_core::CodecError;

fn surface_references() -> Vec<FeatureSurfaceConstructionReference> {
    (0..14)
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

fn surface_file() -> Vec<u8> {
    let part =
        crate::test_support::test_om::composed_feature_history_payload(&[], &[b"abc", b"def"]);
    crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)])
}

fn assert_surface_rejection_releases_storage(references: &[FeatureSurfaceConstructionReference]) {
    let file = surface_file();
    let container = crate::test_support::with_decode_context(|ctx| {
        let container = crate::container::scan_bytes(ctx, &file)?;
        super::offset_data_block_bytes(ctx, &container)?;
        Ok::<_, CodecError>(container)
    })
    .unwrap();
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 65_536;
        },
        |ctx| {
            let rows = feature_surface_construction_payloads(ctx, &container, references)?;
            assert!(rows.is_empty());
            assert_eq!(ctx.resource_refusal(), None);
            let _checkpoint =
                ctx.reserve_scoped(65_536, "test rejected surface scratch release")?;
            Ok::<_, CodecError>(())
        },
    )
    .unwrap();
}

#[test]
fn surface_missing_reference_releases_source_copies() {
    let mut references = surface_references();
    references[13].data_block = None;
    assert_surface_rejection_releases_storage(&references);
}

#[test]
fn surface_missing_block_releases_source_copies() {
    let mut references = surface_references();
    references[13].data_block = Some("nx:om-data-blocks-0:block#99".into());
    assert_surface_rejection_releases_storage(&references);
}

#[test]
fn surface_mixed_owners_release_source_copies() {
    let mut references = surface_references();
    references[13].data_block = Some("nx:om-data-blocks-1:block#1".into());
    assert_surface_rejection_releases_storage(&references);
}

#[test]
fn surface_missing_operation_key_releases_candidate_content() {
    let mut references = surface_references();
    for reference in &mut references {
        reference.operation_label = "unkeyed".into();
    }
    assert_surface_rejection_releases_storage(&references);
}

#[test]
fn surface_output_growth_excludes_consumed_source_slots() {
    let references = surface_references();
    let file = surface_file();
    let container = crate::test_support::with_decode_context(|ctx| {
        let container = crate::container::scan_bytes(ctx, &file)?;
        super::offset_data_block_bytes(ctx, &container)?;
        Ok::<_, CodecError>(container)
    })
    .unwrap();
    let groups = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        "test surface group scratch checkpoint",
        |ctx| {
            let (groups, _storage) = ctx.collect_scoped_btree_groups(
                references
                    .iter()
                    .map(|reference| (reference.operation_label.as_str(), reference)),
                "test surface reference groups",
            )?;
            ctx.reserve_scoped(65_536, "test surface group scratch checkpoint")?;
            Ok(groups)
        },
    );
    let CodecError::ResourceLimit(groups) = groups else {
        panic!("group checkpoint refusal")
    };
    let rows = crate::test_support::with_decode_context(|ctx| {
        feature_surface_construction_payloads(ctx, &container, &references)
    })
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].content.blocks().len(), 14);
    let expected = rows.capacity()
        * std::mem::size_of::<super::FeatureSurfaceConstructionPayload>()
        + rows[0].id.len()
        + rows[0].operation_label.len()
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
        - rows.capacity() * std::mem::size_of::<super::FeatureSurfaceConstructionPayload>()
        - rows[0].operation_label.len();
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::MaterializedBytes,
        "NX surface construction payloads",
        |ctx| {
            let (rows, _storage) = ctx
                .with_scoped_storage("test surface output candidate", || {
                    feature_surface_construction_payloads(ctx, &container, &references)
                })?;
            Ok(rows)
        },
    );
    // Group scratch and surviving child strings precede output-vector growth.
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.used == groups.used + u64_from_index(children_before_growth)
            && limit.additional == u64_from_index(4 * std::mem::size_of::<super::FeatureSurfaceConstructionPayload>())));
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "test surface retained checkpoint",
        |ctx| {
            let rows = feature_surface_construction_payloads(ctx, &container, &references)?;
            ctx.charge_retained(1, "test surface retained checkpoint")?;
            Ok(rows)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.used == u64_from_index(expected) && limit.additional == 1));
}
