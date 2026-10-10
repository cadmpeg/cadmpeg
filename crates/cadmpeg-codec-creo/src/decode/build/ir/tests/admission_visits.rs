// SPDX-License-Identifier: Apache-2.0
use super::super::{
    admitted_display_strips, pattern_kind_has_unresolved_operands, transfer_display_tessellations,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::patterns::{
    CompositePattern, PatternKind, PatternStage, PatternTransform, StagePatternKind,
};

fn resolved_stage() -> StagePatternKind {
    StagePatternKind::new(PatternTransform::Linear {
        direction: Some(
            cadmpeg_ir::features::FeatureDirection3::new([1.0, 0.0, 0.0].into())
                .expect("direction"),
        ),
        spacing: cadmpeg_ir::scalar::PositiveLength::new(1.0).expect("spacing"),
        count: 1,
        second: None,
    })
    .expect("resolved single occurrence")
}

fn fixed_work_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    policy
}

#[test]
fn fixed_pattern_coverage_is_free_and_preserves_original_refusal() {
    let stage = resolved_stage();
    let pattern = stage.clone().widen::<CompositePattern>();
    let unresolved: PatternKind = PatternKind::UNRESOLVED;
    let arena = DecodeArena::new();
    let policy = fixed_work_policy(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(!pattern_kind_has_unresolved_operands(&ctx, &pattern).expect("fixed resolved pattern"));
    assert!(!pattern_kind_has_unresolved_operands(&ctx, &stage).expect("fixed resolved stage"));
    assert!(
        pattern_kind_has_unresolved_operands(&ctx, &unresolved).expect("fixed unresolved pattern")
    );
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx
        .charge_work_limit(1, "after fixed pattern coverage")
        .expect_err("zero cap");
    assert_eq!(
        (original.dimension, original.used, original.additional),
        (ResourceDimension::WorkUnits, 0, 1)
    );
    for result in [
        pattern_kind_has_unresolved_operands(&ctx, &pattern),
        pattern_kind_has_unresolved_operands(&ctx, &stage),
        pattern_kind_has_unresolved_operands(&ctx, &unresolved),
    ] {
        assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn composite_pattern_coverage_visits_present_stages_until_first_unresolved() {
    let mut first = vec![StagePatternKind::UNRESOLVED];
    first.extend(std::iter::repeat_n(resolved_stage(), 128));
    let mut second = vec![resolved_stage(), StagePatternKind::UNRESOLVED];
    second.extend(std::iter::repeat_n(resolved_stage(), 128));
    for (stages, visits, expected) in [
        (vec![resolved_stage()], 1, false),
        (vec![resolved_stage(), resolved_stage()], 2, false),
        (first, 1, true),
        (second, 2, true),
    ] {
        let stages = stages
            .into_iter()
            .map(|pattern| PatternStage {
                pattern: Box::new(pattern),
            })
            .collect();
        let pattern: PatternKind = PatternKind::new(PatternTransform::Composite {
            stages: CompositePattern::new(stages).expect("nonempty composable stages"),
        })
        .expect("composite pattern");
        crate::test_support::assert_refusal_order(
            ResourceDimension::WorkUnits,
            &vec![
                "creo pattern composite stage traversal";
                usize::try_from(visits).expect("fixture visit count")
            ],
            |cap| {
                let arena = DecodeArena::new();
                let policy = fixed_work_policy(cap);
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let result = pattern_kind_has_unresolved_operands(&ctx, &pattern);
                let original = if cap == visits {
                    assert_eq!(result.expect("present stage visits admitted"), expected);
                    assert_eq!(ctx.resource_refusal(), None);
                    let original = ctx
                        .charge_work_limit(1, "after composite pattern coverage")
                        .expect_err("exact visits");
                    assert_eq!(
                        (original.dimension, original.used, original.additional),
                        (ResourceDimension::WorkUnits, visits, 1)
                    );
                    original
                } else {
                    let original = ctx.resource_refusal().expect("stage visit refusal");
                    assert!(
                        matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                    assert_eq!(
                        (
                            original.dimension,
                            original.limit,
                            original.used,
                            original.additional,
                            original.operation
                        ),
                        (
                            ResourceDimension::WorkUnits,
                            cap,
                            cap,
                            1,
                            "creo pattern composite stage traversal"
                        )
                    );
                    original
                };
                assert!(
                    matches!(pattern_kind_has_unresolved_operands(&ctx, &pattern),
                Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                assert_eq!(ctx.resource_refusal(), Some(original));
                if cap == visits {
                    Ok(())
                } else {
                    Err(original.into())
                }
            },
        );
    }
}

#[test]
fn empty_display_transfer_and_span_queries_are_free_and_preserve_refusal() {
    let scan = crate::test_support::empty_container_scan();
    let arena = DecodeArena::new();
    let policy = fixed_work_policy(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    transfer_display_tessellations(&ctx, &scan, &mut ir, &mut annotations).expect("empty display");
    assert!(ir.model.tessellations.is_empty());
    for vertices in [Vec::new(), vec![0u8, 1, 2]] {
        assert!(admitted_display_strips(&ctx, vertices, &[])
            .expect("no spans")
            .is_none());
    }
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx
        .charge_work_limit(1, "after empty display queries")
        .expect_err("zero cap");
    assert_eq!(
        (original.dimension, original.used, original.additional),
        (ResourceDimension::WorkUnits, 0, 1)
    );
    assert!(
        matches!(transfer_display_tessellations(&ctx, &scan, &mut ir, &mut annotations),
        Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert!(
        matches!(admitted_display_strips(&ctx, Vec::<u8>::new(), &[]),
        Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn unavailable_display_span_stops_before_unneeded_tail() {
    let mut spans = vec![u32::MAX];
    spans.extend(std::iter::repeat_n(3, 128));
    crate::test_support::assert_refusal_order(
        ResourceDimension::WorkUnits,
        &["creo display strip span traversal"],
        |cap| {
            let arena = DecodeArena::new();
            let policy = fixed_work_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = admitted_display_strips(&ctx, vec![0u8, 1], &spans);
            let original = if cap == 1 {
                assert!(result
                    .expect("only the first unsupported span is visited")
                    .is_none());
                assert_eq!(ctx.resource_refusal(), None);
                let original = ctx
                    .charge_work_limit(1, "after unavailable display span")
                    .expect_err("exact visit");
                assert_eq!(
                    (original.dimension, original.used, original.additional),
                    (ResourceDimension::WorkUnits, 1, 1)
                );
                original
            } else {
                let original = ctx.resource_refusal().expect("first span visit refused");
                assert!(
                    matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                );
                assert_eq!(
                    (
                        original.dimension,
                        original.used,
                        original.additional,
                        original.operation
                    ),
                    (
                        ResourceDimension::WorkUnits,
                        0,
                        1,
                        "creo display strip span traversal"
                    )
                );
                original
            };
            assert!(
                matches!(admitted_display_strips(&ctx, vec![0u8, 1], &spans),
            Err(CodecError::ResourceLimit(actual)) if actual == original)
            );
            assert_eq!(ctx.resource_refusal(), Some(original));
            if cap == 1 {
                Ok(())
            } else {
                Err(original.into())
            }
        },
    );
}

fn check_display_staging_peak(shaded: bool) {
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::tessellation::{Strip, Strips, Tessellation, TessellationMesh};
    let mut scan = super::inch_strip(vec![[1.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 4.0]]);
    if shaded {
        scan.primitives.triangle_strips[0] = crate::decode::with_test_decode_ctx(|ctx| {
            crate::primdata::PrimitiveTriangleStrip::new(
                ctx,
                0,
                scan.primitives.triangle_strips[0]
                    .positions()
                    .copied()
                    .collect(),
                Some(vec![
                    cadmpeg_ir::units::FiniteVector::new([0.0, 0.0, 1.0])
                        .expect("normal");
                    3
                ]),
                vec![3],
            )
        })
        .expect("fixture service")
        .expect("legal shaded strip");
    }
    let initial_ir = || {
        let mut ir = CadIr::empty();
        ir.model.tessellations = Vec::with_capacity(4);
        for offset in 100..104 {
            let id = cadmpeg_ir::tessellation::TessellationId::try_from(format!(
                "creo:solid_primdata:tessellation#{offset}"
            ))
            .expect("distinct fixture identity");
            let strip = Strip::new(vec![
                FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).expect("point"),
                FinitePoint3::new(Point3::new(0.0, 1.0, 0.0)).expect("point"),
                FinitePoint3::new(Point3::new(0.0, 0.0, 1.0)).expect("point"),
            ])
            .expect("three vertices");
            let mesh = TessellationMesh::Strips {
                strips: Strips::new(vec![strip]).expect("one strip"),
            };
            ir.model
                .tessellations
                .push(Tessellation::from_parts(id, mesh, Vec::new()).expect("fixture mesh"));
        }
        assert_eq!(ir.model.tessellations.capacity(), 4);
        ir
    };
    let below = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo model tessellations"),
        |cap| {
            let mut ir = initial_ir();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            transfer_display_tessellations(
                &ctx,
                &scan,
                &mut ir,
                &mut cadmpeg_ir::AnnotationBuilder::new(),
            )
        },
    );
    let peak = below.checked_add(1).expect("model growth boundary");
    for cap in [below, peak] {
        let mut ir = initial_ir();
        let previous = ir.model.tessellations.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let result = transfer_display_tessellations(&ctx, &scan, &mut ir, &mut annotations);
        assert_eq!(&ir.model.tessellations[..4], previous.as_slice());
        let original = if cap == peak {
            result.expect("sequential staging and model growth fit their larger peak");
            assert_eq!(ir.model.tessellations.len(), 5);
            let added = &ir.model.tessellations[4];
            assert_eq!(added.id.as_str(), "creo:solid_primdata:tessellation#0");
            assert_eq!(
                added.vertices(),
                vec![
                    FinitePoint3::new(Point3::new(25.4, 0.0, 0.0)).expect("millimeters"),
                    FinitePoint3::new(Point3::new(0.0, 50.8, 0.0)).expect("millimeters"),
                    FinitePoint3::new(Point3::new(0.0, 0.0, 101.6)).expect("millimeters"),
                ]
            );
            assert_eq!(
                matches!(added.mesh(), TessellationMesh::ShadedStrips { .. }),
                shaded
            );
            assert_eq!(ctx.resource_refusal(), None);
            let released = ctx
                .reserve_scoped(peak, "after display staging and model growth")
                .expect("both scratch peaks released");
            drop(released);
            ctx.charge_work_limit(policy.limits.max_work_units, "after display transfer")
                .expect_err("seed sticky refusal")
        } else {
            let original = ctx.resource_refusal().expect("model overlap refused");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!(ir.model.tessellations.len(), 4);
            assert_eq!(
                (
                    original.dimension,
                    original.limit,
                    original.used,
                    original.additional,
                    original.operation
                ),
                (
                    ResourceDimension::MaterializedBytes,
                    cap,
                    0,
                    peak,
                    "creo model tessellations"
                )
            );
            original
        };
        let retained_count = ir.model.tessellations.len();
        assert!(
            matches!(transfer_display_tessellations(&ctx, &scan, &mut ir, &mut annotations),
            Err(CodecError::ResourceLimit(actual)) if actual == original)
        );
        assert_eq!(ir.model.tessellations.len(), retained_count);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert_eq!(
            scan.primitives.triangle_strips[0]
                .positions()
                .next()
                .expect("source vertex")
                .get(),
            [1.0, 0.0, 0.0]
        );
    }
}

#[test]
fn unshaded_display_staging_releases_before_model_growth() {
    check_display_staging_peak(false);
}

#[test]
fn shaded_display_staging_releases_before_model_growth() {
    check_display_staging_peak(true);
}
