// SPDX-License-Identifier: Apache-2.0

#[test]
fn draft_text_projection_copies_the_checked_value() {
    let (file, payload) = super::draft_graph_fixture_with_content(b"\x66\x32\x03\x03A\0");
    let container =
        crate::test_support::with_decode_context(|ctx| crate::container::scan_bytes(ctx, &file))
            .expect("synthetic container");
    let rows = crate::test_support::with_decode_context(|ctx| {
        super::feature_draft_construction_graph_strings(
            ctx,
            &container,
            std::slice::from_ref(&payload),
        )
    })
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(serde_json::to_value(&rows[0]).unwrap()["value"], "A");
    let error = crate::test_support::resource_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "NX draft construction string",
        |ctx| {
            super::feature_draft_construction_graph_strings(
                ctx,
                &container,
                std::slice::from_ref(&payload),
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.additional == 1)
    );
}
