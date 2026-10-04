//! Adjacent identity deduplication output and refusal.

#[test]
fn typed_relations_identity_deduplication_preserves_output_and_refusal() {
    use cadmpeg_ir::sketches::{SketchEntityId, SketchLocus};
    let first = SketchEntityId::mint("synthetic:test:id#a-first").unwrap();
    let second = SketchEntityId::mint("synthetic:test:id#z-second").unwrap();
    let input = [SketchLocus::Entity(second.clone()), SketchLocus::Entity(first.clone()), SketchLocus::Entity(second.clone())];
    let sort = |ctx: &cadmpeg_core::decode::DecodeContext<'_>, values: &mut Vec<SketchLocus>| {
        super::super::sort_axis_relation_point_loci(ctx, values)
    };
    let mut values = input.to_vec();
    sort(&cadmpeg_test_support::service_decode_context(), &mut values).unwrap();
    assert_eq!(values, [SketchLocus::Entity(first), SketchLocus::Entity(second)]);
    crate::test_support::work_refusal_at("deduplicate SLDPRT axis relation point loci", |ctx| {
        sort(ctx, &mut input.to_vec())
    });
}
