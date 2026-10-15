// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn projected_curve_text_releases_parser_storage() {
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[],
        &[b"\x66\x32\x03\x05ABC\0", b""],
    );
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, &file))
            .unwrap();
    let payload = crate::test_support::with_decode_context(|ctx| {
        let blocks = super::offset_data_block_bytes(ctx, &container)?;
        let id = blocks
            .keys()
            .find(|id| id.ends_with(":block#1"))
            .unwrap()
            .clone();
        let content = super::FeaturePayloadContent::from_source(ctx, [id], blocks)?.unwrap();
        Ok::<_, CodecError>(super::FeatureConstructionPayload {
            id: "payload".into(),
            operation_label: "operation".into(),
            owner: super::FeatureConstructionOwner::ProjectedCurve {
                operation_kind: super::FeatureProjectedCurveKind::Projected,
                construction_references: Vec::new(),
            },
            content,
        })
    })
    .unwrap();
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::feature_projected_curve_construction_strings(
            ctx,
            &container,
            std::slice::from_ref(&payload),
        )
    };
    let rows = crate::test_support::with_decode_context(|ctx| decode(ctx)).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].value.as_str(), "ABC");
    let expected = rows.capacity()
        * std::mem::size_of::<super::FeatureProjectedCurveConstructionString>()
        + rows
            .iter()
            .map(|row| {
                row.id.len()
                    + row.operation_label.len()
                    + row.construction_payload.len()
                    + row.value.as_str().len()
            })
            .sum::<usize>();
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "test projected_curve_text retained checkpoint",
        |ctx| {
            let rows = decode(ctx)?;
            ctx.charge_retained(1, "test projected_curve_text retained checkpoint")?;
            Ok(rows)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.used == u64_from_index(expected) && limit.additional == 1));
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_materialized_bytes = 1_048_576,
        |ctx| {
            let rows = decode(ctx)?;
            assert_eq!(rows.len(), 1);
            let _checkpoint =
                ctx.reserve_scoped(1_048_576, "test projected_curve_text scratch checkpoint")?;
            Ok::<_, CodecError>(())
        },
    )
    .unwrap();
}

#[test]
fn surface_text_releases_parser_storage() {
    let part = crate::test_support::test_om::composed_feature_history_payload(
        &[],
        &[b"\x66\x1b\x03\x03ABC\0", b""],
    );
    let file =
        crate::test_support::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, &file))
            .unwrap();
    let payload = crate::test_support::with_decode_context(|ctx| {
        let blocks = super::offset_data_block_bytes(ctx, &container)?;
        let id = blocks
            .keys()
            .find(|id| id.ends_with(":block#1"))
            .unwrap()
            .clone();
        let content = super::FeaturePayloadContent::from_source_array(
            ctx,
            std::array::from_fn::<_, 14, _>(|_| id.clone()),
            blocks,
        )?
        .unwrap();
        Ok::<_, CodecError>(super::FeatureSurfaceConstructionPayload {
            id: "payload".into(),
            operation_label: "operation".into(),
            construction_references: std::array::from_fn(|_| "reference".into()),
            content,
        })
    })
    .unwrap();
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::feature_surface_construction_strings(ctx, &container, std::slice::from_ref(&payload))
    };
    let rows = crate::test_support::with_decode_context(|ctx| decode(ctx)).unwrap();
    assert_eq!(rows.len(), 14);
    assert!(rows.iter().all(|row| row.value.as_str() == "ABC"));
    let expected = rows.capacity() * std::mem::size_of::<super::FeatureSurfaceConstructionString>()
        + rows
            .iter()
            .map(|row| {
                row.id.len()
                    + row.operation_label.len()
                    + row.surface_construction_payload.len()
                    + row.value.as_str().len()
            })
            .sum::<usize>();
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "test surface_text retained checkpoint",
        |ctx| {
            let rows = decode(ctx)?;
            ctx.charge_retained(1, "test surface_text retained checkpoint")?;
            Ok(rows)
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.used == u64_from_index(expected) && limit.additional == 1));
    crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_materialized_bytes = 1_048_576,
        |ctx| {
            let rows = decode(ctx)?;
            assert_eq!(rows.len(), 14);
            let _checkpoint =
                ctx.reserve_scoped(1_048_576, "test surface_text scratch checkpoint")?;
            Ok::<_, CodecError>(())
        },
    )
    .unwrap();
}
