// SPDX-License-Identifier: Apache-2.0

use crate::geometry::curve_payloads::{
    SilhouetteCurveConstruction, SpringCurvePayload, SurfaceOffsetCurveConstruction,
    ThreeSurfaceIntersectionCurvePayload, TwoSidedOffsetCurveConstruction,
};
use crate::geometry::pcurve::{LinePcurve, PcurveGeometry};
use crate::geometry::{
    CacheContract, CacheFirstCurveForm, CacheFirstCurveParameterization, IntcurveSupportContext,
    IntcurveSupportSide, ProceduralCurveDefinition, RevisionCacheForm, SilhouetteKind,
    SpringLayout, SupportPcurve,
};
use crate::ids::{CurveId, SurfaceId};
use crate::math::{Point2, Vector3};
use crate::scalar::{FiniteReal, PositiveI64};

fn line_side(origin: Point2, direction: Point2) -> IntcurveSupportSide {
    IntcurveSupportSide {
        surface: None,
        pcurve: Some(SupportPcurve::new(
            PcurveGeometry::Line(LinePcurve::try_new(origin, direction).expect("line")),
            None,
        )),
    }
}

/// The direction of a side's line pcurve.
fn line_direction(side: &IntcurveSupportSide) -> Point2 {
    match &side.pcurve.as_ref().expect("pcurve").geometry {
        PcurveGeometry::Line(line) => *line.direction().as_raw(),
        _ => unreachable!("line pcurves only"),
    }
}

fn context() -> IntcurveSupportContext {
    IntcurveSupportContext::try_new(
        [
            line_side(Point2::new(0.5, 1.0), Point2::new(1.0, -0.5)),
            line_side(Point2::new(2.0, 0.0), Point2::new(0.0, 1.0)),
        ],
        [1.0, 4.0],
        [vec![2.0], vec![], vec![]],
    )
    .expect("context")
}

fn definitions() -> Vec<ProceduralCurveDefinition> {
    let form = CacheFirstCurveForm {
        revision: PositiveI64::new(22_800).expect("revision"),
        cache: RevisionCacheForm::Parameterization(CacheFirstCurveParameterization {
            interval: [Some(1.0), Some(4.0)],
            closed_form: 0,
        }),
        support_bounds: [[None; 4]; 2],
        solved_range: [Some(1.5), None],
        extension: 0,
    };
    let mut spring_form = form.clone();
    spring_form.support_bounds[0] = [Some(-2.0), Some(3.0), Some(1.0), Some(5.0)];
    vec![
        ProceduralCurveDefinition::ThreeSurfaceIntersection(
            ThreeSurfaceIntersectionCurvePayload::try_new(
                context(),
                1,
                line_side(Point2::new(-1.0, 3.0), Point2::new(0.25, 0.75)),
            )
            .expect("three-surface"),
        ),
        ProceduralCurveDefinition::TwoSidedOffset(
            TwoSidedOffsetCurveConstruction::try_new(context(), false, [1.0, -1.0], None)
                .expect("two-sided offset"),
        ),
        ProceduralCurveDefinition::Silhouette(
            SilhouetteCurveConstruction::try_new(
                context(),
                SilhouetteKind::Standard {},
                SurfaceId::mint("test:model:surface#cast").expect("surface"),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .expect("silhouette"),
        ),
        ProceduralCurveDefinition::Spring(
            SpringCurvePayload::try_new(
                SpringLayout::CacheFirst {
                    context: context(),
                    form: spring_form,
                },
                0,
            )
            .expect("cache-first spring"),
        ),
        ProceduralCurveDefinition::SurfaceOffset(
            SurfaceOffsetCurveConstruction::try_new(
                context(),
                false,
                [[0.0, 1.0], [0.0, 2.0]],
                (
                    CurveId::mint("test:model:curve#base").expect("curve"),
                    [0.0, 3.0],
                    [Some(0.25), Some(2.75)],
                ),
                CacheContract::Revision { form },
                0.25,
                [0.5, 2.0],
            )
            .expect("surface offset"),
        ),
    ]
}

/// Each family restates its context on `-t`: the interval `[1, 4]` becomes
/// `[-4, -1]`, the discontinuity `2` becomes `-2`, and each line pcurve runs
/// in the opposite direction. A second reversal restores the definition.
#[test]
fn context_families_reverse_about_zero_and_back() {
    let assert_context = |context: &IntcurveSupportContext| {
        assert_eq!(context.parameter_range().endpoints(), [-4.0, -1.0]);
        assert_eq!(
            FiniteReal::raw_lanes(context.discontinuities()),
            [vec![-2.0], vec![], vec![]]
        );
        assert_eq!(line_direction(&context.sides()[0]), Point2::new(-1.0, 0.5));
        assert_eq!(line_direction(&context.sides()[1]), Point2::new(-0.0, -1.0));
    };
    for original in definitions() {
        let mut reversed = original.clone();
        reversed.reverse_support_contexts().expect("reversible");
        match &reversed {
            ProceduralCurveDefinition::ThreeSurfaceIntersection(payload) => {
                assert_context(payload.context());
                assert_eq!(line_direction(payload.third()), Point2::new(-0.25, -0.75));
                assert_eq!(payload.selector(), &1);
            }
            ProceduralCurveDefinition::TwoSidedOffset(payload) => assert_context(payload.context()),
            ProceduralCurveDefinition::Silhouette(payload) => assert_context(payload.context()),
            ProceduralCurveDefinition::Spring(payload) => {
                assert_context(payload.support_context());
                let SpringLayout::CacheFirst { context, form } = payload.layout() else {
                    panic!("cache-first spring")
                };
                assert_eq!(context, payload.support_context());
                assert_eq!(form.solved_range, [None, FiniteReal::new(-1.5)]);
                assert_eq!(
                    form.support_bounds[0].map(|bound| bound.map(FiniteReal::get)),
                    [Some(-2.0), Some(3.0), Some(1.0), Some(5.0)]
                );
                assert!(matches!(
                    form.cache,
                    RevisionCacheForm::Parameterization(CacheFirstCurveParameterization {
                        interval,
                        ..
                    }) if interval == [FiniteReal::new(-4.0), FiniteReal::new(-1.0)]
                ));
                assert_eq!(payload.direction(), &0);
                let restored: ProceduralCurveDefinition = serde_json::from_value(
                    serde_json::to_value(&reversed).expect("serialize spring"),
                )
                .expect("deserialize spring");
                assert_eq!(restored, reversed);
            }
            ProceduralCurveDefinition::SurfaceOffset(payload) => {
                assert_context(payload.context());
                assert_eq!(
                    payload.base_endpoints(),
                    [FiniteReal::new(2.75), FiniteReal::new(0.25)]
                );
                let form = payload.cache_first().expect("cache-first form");
                assert_eq!(form.solved_range, [None, FiniteReal::new(-1.5)]);
                assert!(matches!(
                    form.cache,
                    RevisionCacheForm::Parameterization(CacheFirstCurveParameterization {
                        interval,
                        ..
                    }) if interval == [FiniteReal::new(-4.0), FiniteReal::new(-1.0)]
                ));
                assert_eq!(payload.base_range().endpoints(), [0.0, 3.0]);
                assert_eq!([payload.shift().get(), payload.scale().get()], [0.5, 2.0]);
            }
            _ => unreachable!("only context families are built"),
        }
        reversed.reverse_support_contexts().expect("reversible");
        assert_eq!(reversed, original);
    }
}

#[test]
fn spring_reversal_failure_preserves_both_contexts_and_the_form() {
    use crate::geometry::pcurve::OffsetPcurve;

    let original = definitions()
        .into_iter()
        .find(|definition| matches!(definition, ProceduralCurveDefinition::Spring(_)))
        .expect("spring definition");
    let ProceduralCurveDefinition::Spring(payload) = original else {
        panic!("spring")
    };
    let mut layout = payload.layout().to_raw();
    let SpringLayout::CacheFirst { context: own, .. } = &mut layout else {
        panic!("cache-first spring")
    };
    let mut sides = own.sides().clone();
    let pcurve = sides[1].pcurve.as_mut().expect("second pcurve");
    pcurve.geometry = PcurveGeometry::Offset(
        OffsetPcurve::try_new(0.25, Box::new(pcurve.geometry.clone())).expect("offset pcurve"),
    );
    *own = IntcurveSupportContext::try_new(
        sides,
        own.parameter_range().endpoints(),
        FiniteReal::raw_lanes(own.discontinuities()),
    )
    .expect("context with an offset pcurve");
    let mut definition = ProceduralCurveDefinition::Spring(
        SpringCurvePayload::try_new(layout, 0).expect("spring with an offset pcurve"),
    );
    let original = definition.clone();
    assert_eq!(definition.reverse_support_contexts(), None);
    assert_eq!(definition, original);
}
