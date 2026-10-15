// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn om_text_releases_parser_storage() {
    let file = crate::test_support::test_prt::prt_with_indexed_om_section();
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, &file))
            .unwrap();
    let decode =
        |ctx: &cadmpeg_core::decode::DecodeContext<'_>| super::string_values(ctx, &container);
    let rows = crate::test_support::with_decode_context(|ctx| decode(ctx)).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].value.as_str(), "SKETCH_001");
    let expected = rows.capacity() * std::mem::size_of::<super::StringValue>()
        + rows
            .iter()
            .map(|row| {
                row.id.len() + row.record.len() + row.source_entry.len() + row.value.as_str().len()
            })
            .sum::<usize>();
    let error = crate::test_support::resource_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "test om_text retained checkpoint",
        |ctx| {
            let rows = decode(ctx)?;
            ctx.charge_retained(1, "test om_text retained checkpoint")?;
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
            let _checkpoint = ctx.reserve_scoped(1_048_576, "test om_text scratch checkpoint")?;
            Ok::<_, CodecError>(())
        },
    )
    .unwrap();
}
