//! Adjacent identity deduplication output and refusal.

#[test]
fn transforms_identity_deduplication_preserves_output_and_refusal() {
    use cadmpeg_ir::sketches::SketchEntityId;
    let first = SketchEntityId::mint("synthetic:test:id#a-first").unwrap();
    let second = SketchEntityId::mint("synthetic:test:id#z-second").unwrap();
    let input = [second.clone(), first.clone(), second.clone()];
    let sort = |ctx: &cadmpeg_core::decode::DecodeContext<'_>, values: &mut Vec<SketchEntityId>| {
        super::super::sort_marker_entity_ids(ctx, values, "sort SLDPRT test marker identities")
    };
    let mut values = input.to_vec();
    sort(&cadmpeg_test_support::service_decode_context(), &mut values).unwrap();
    assert_eq!(values, [first, second]);
    crate::test_support::work_refusal_at("deduplicate SLDPRT marker entity identities", |ctx| {
        sort(ctx, &mut input.to_vec())
    });
}
