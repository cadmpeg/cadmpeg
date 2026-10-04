//! Adjacent identity deduplication output and refusal.

#[test]
fn relation_loci_identity_deduplication_preserves_output_and_refusal() {
    use cadmpeg_ir::sketches::{SketchEntityId, SketchLocus};
    let first = SketchEntityId::mint("synthetic:test:id#a-first").unwrap();
    let second = SketchEntityId::mint("synthetic:test:id#z-second").unwrap();
    let input = [SketchLocus::Entity(second.clone()), SketchLocus::Entity(first.clone()), SketchLocus::Entity(second.clone())];
    let sort = |ctx: &cadmpeg_core::decode::DecodeContext<'_>, values: &mut Vec<SketchLocus>| {
        super::super::sort_profile_loci(ctx, values, "sort SLDPRT test profile loci")
    };
    let mut values = input.to_vec();
    sort(&cadmpeg_test_support::service_decode_context(), &mut values).unwrap();
    assert_eq!(values, [SketchLocus::Entity(first), SketchLocus::Entity(second)]);
    crate::test_support::work_refusal_at("deduplicate SLDPRT profile loci", |ctx| {
        sort(ctx, &mut input.to_vec())
    });
}

struct Lines {
    sketch: cadmpeg_ir::sketches::SketchId,
    entities: Vec<cadmpeg_ir::sketches::SketchEntity>,
    loci: std::collections::HashMap<String, Vec<cadmpeg_ir::sketches::SketchLocus>>,
}

fn lines() -> Lines {
    use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry,
        SketchGeometryDefinition, SketchId, SketchLocus};
    let sketch = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let entities: Vec<_> = ["a", "z"].map(|suffix| {
        SketchEntity::new(SketchEntityId::mint(format!("synthetic:test:id#{suffix}")).unwrap(),
            sketch.clone(), SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                end: cadmpeg_ir::math::Point2::new(1.0, 0.0),
            }).unwrap()).with_native_ref(Some("root".into()))
    }).into();
    let loci = std::collections::HashMap::from([("root".into(),
        entities.iter().map(|entity| SketchLocus::Entity(entity.id().clone())).collect())]);
    Lines { sketch, entities, loci }
}

#[test]
fn dynamic_marker_line_deduplication_preserves_candidates_and_refusal() {
    let fixture = lines();
    let markers = std::collections::HashMap::new();
    let solve = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::dynamic_marker_line_candidates(ctx, "root", &markers,
            &fixture.loci, &fixture.entities)
    };
    assert_eq!(solve(&cadmpeg_test_support::service_decode_context()).unwrap(),
        fixture.entities.iter().map(|entity| entity.id().clone()).collect::<Vec<_>>());
    crate::test_support::work_refusal_at("deduplicate SLDPRT marker line candidates", solve);
}

#[test]
fn dynamic_line_operand_deduplication_preserves_candidates_and_refusal() {
    use crate::records::{FeatureInputOperand, FeatureInputOperandKind,
        FeatureInputRelationFamily, FeatureInputRelationInstance};
    let fixture = lines();
    let markers = std::collections::HashMap::new();
    let relation = FeatureInputRelationInstance {
        id: "relation".into(), parent: "lane".into(), ordinal: 0, offset: 0,
        family: FeatureInputRelationFamily::LineLineDistance, class_ref: "class".into(),
        feature_ref: "feature".into(),
        scalars: crate::records::relation_scalars::RelationScalars::from_refs(
            vec!["scalar".into()], None, None).unwrap(),
        operands: vec![FeatureInputOperand { offset: 0, reference_ref: "reference".into(),
            kind: FeatureInputOperandKind::D6, entity_index: 0, entity_ref: Some("root".into()) }],
    };
    let solve = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::dynamic_line_operand_candidates(ctx, &relation, 0, &fixture.sketch,
            &markers, &fixture.loci, &fixture.entities)
    };
    assert_eq!(solve(&cadmpeg_test_support::service_decode_context()).unwrap(),
        fixture.entities.iter().map(|entity| entity.id().clone()).collect::<Vec<_>>());
    crate::test_support::work_refusal_at("deduplicate SLDPRT marker entities", solve);
}

#[test]
fn single_marker_line_deduplication_preserves_ambiguity_and_refusal() {
    let fixture = lines();
    let markers = std::collections::HashMap::new();
    let solve = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        super::super::single_marker_line_entity(ctx, "root", &markers,
            &fixture.loci, &fixture.entities)
    };
    assert_eq!(solve(&cadmpeg_test_support::service_decode_context()).unwrap(), None);
    crate::test_support::work_refusal_at("deduplicate SLDPRT marker entities", solve);
}
