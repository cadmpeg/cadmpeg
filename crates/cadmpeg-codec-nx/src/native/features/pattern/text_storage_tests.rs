// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn pattern_text_releases_parser_storage() {
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
        let blocks = super::super::offset_data_block_bytes(ctx, &container)?;
        let id = blocks
            .keys()
            .find(|id| id.ends_with(":block#1"))
            .unwrap()
            .clone();
        let content =
            super::super::payload_content::FeaturePayloadContent::from_source(ctx, [id], blocks)?
                .unwrap();
        Ok::<_, CodecError>(super::super::FeatureConstructionPayload {
            id: "payload".into(),
            operation_label: "operation".into(),
            owner: super::FeatureConstructionOwner::Pattern {
                operation_kind: super::FeaturePatternKind::Feature,
                reference_layout: super::PatternPayloadReferenceLayout::CanonicalGraph,
                construction_references: Vec::new(),
            },
            content,
        })
    })
    .unwrap();
    let decode = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::feature_pattern_construction_strings(ctx, &container, std::slice::from_ref(&payload))
    };
    let rows = crate::test_support::with_decode_context(|ctx| decode(ctx)).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].value.as_str(), "ABC");
    let expected = rows.capacity() * std::mem::size_of::<super::FeaturePatternConstructionString>()
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
        "test pattern_text retained checkpoint",
        |ctx| {
            let rows = decode(ctx)?;
            ctx.charge_retained(1, "test pattern_text retained checkpoint")?;
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
                ctx.reserve_scoped(1_048_576, "test pattern_text scratch checkpoint")?;
            Ok::<_, CodecError>(())
        },
    )
    .unwrap();
}
